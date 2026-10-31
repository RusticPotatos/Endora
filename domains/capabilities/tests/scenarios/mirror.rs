//! The node's mirror of the fork, and of upstream beside it (ADR 0106, amended 2026-10-31):
//! `features/the-team-reads-upstream.feature`, run against `endora_capabilities::repo::Mirror`
//! with local repositories for the fork and for upstream, and no network. The sandbox is played
//! by a plain clone of the bundle the node hands out, unpacked as the worker does it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cucumber::{World, given, then, when};
use endora_capabilities::repo::{Bundle, Mirror, REFRESH_MS};

/// One scenario's node: the fork, upstream, the node's mirror and what it last said.
#[derive(Default, World)]
pub struct Node {
    root: PathBuf,
    fork: PathBuf,
    upstream: PathBuf,
    /// Upstream's working copy, where its new commits are made and pushed from.
    upstream_work: PathBuf,
    mirror: Option<Mirror>,
    checkout: Option<Bundle>,
    /// What the node said of the last thing asked: what it took, or why it refused.
    said: Option<Result<String, String>>,
    /// The head of the last branch taken.
    taken: Option<(String, String)>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("root", &self.root)
            .field("said", &self.said)
            .finish_non_exhaustive()
    }
}

const JOHN: [&str; 4] = [
    "-c",
    "user.name=John Doe",
    "-c",
    "user.email=john@example.com",
];
const HQ: [&str; 4] = [
    "-c",
    "user.name=Endora HQ",
    "-c",
    "user.email=hq@endora.invalid",
];
const TRAILER: &str = "Made-by: Endora HQ (Ada) for task #936";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn commit(dir: &Path, who: [&str; 4], file: &str, body: &str, message: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", "."]);
    let mut args: Vec<&str> = who.to_vec();
    args.extend(["commit", "-q", "-m", message]);
    git(dir, &args);
}

impl Node {
    fn mirror(&self) -> &Mirror {
        self.mirror.as_ref().expect("the node keeps a mirror")
    }

    /// The node's mirror of the fork, with upstream at `upstream` beside it.
    fn keep(&mut self, upstream: &Path) {
        self.mirror = Some(
            Mirror::new(
                &self.root.join("data"),
                self.fork.to_str().unwrap(),
                "develop",
            )
            .with_upstream(upstream.to_str().unwrap()),
        );
    }

    fn check_out(&mut self, now: i64) -> Bundle {
        let bundle = self
            .mirror()
            .bundle(None, now)
            .expect("the fork checks out");
        self.checkout = Some(bundle.clone());
        bundle
    }

    /// The sandbox: the checkout unpacked as the worker does it, upstream's refs as
    /// `upstream/<branch>`, and no remote.
    fn sandbox(&self, name: &str) -> PathBuf {
        let bundle = self.checkout.as_ref().expect("something was checked out");
        let file = self.root.join(format!("{name}.bundle"));
        std::fs::write(&file, &bundle.bytes).unwrap();
        let repo = self.root.join(name);
        let _ = std::fs::remove_dir_all(&repo);
        git(
            &self.root,
            &[
                "clone",
                "-q",
                "-b",
                &bundle.branch,
                file.to_str().unwrap(),
                name,
            ],
        );
        git(
            &repo,
            &[
                "fetch",
                "-q",
                file.to_str().unwrap(),
                "refs/upstream/*:refs/remotes/upstream/*",
            ],
        );
        git(&repo, &["remote", "remove", "origin"]);
        repo
    }

    /// Bundles `branch` from develop in the sandbox and hands it to the node.
    fn submit(&mut self, repo: &Path, branch: &str) {
        let file = self.root.join("submit.bundle");
        git(
            repo,
            &[
                "bundle",
                "create",
                "-q",
                file.to_str().unwrap(),
                &format!("develop..{branch}"),
            ],
        );
        let bytes = std::fs::read(&file).unwrap();
        let said = self.mirror().submit(&bytes, branch, 936, 1);
        if let Ok(taken) = &said {
            self.taken = Some((taken.branch.clone(), taken.head.clone()));
        }
        self.said = Some(said.map(|t| t.title()));
    }
}

#[given(expr = "the owner's fork of ansible-nas, worked from {string}")]
fn the_fork(w: &mut Node, base: String) {
    let root = std::env::temp_dir().join(format!(
        "endora-upstream-scenario-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("seed")).unwrap();
    let seed = root.join("seed");
    git(&seed, &["init", "-q", "-b", &base]);
    commit(&seed, JOHN, "nas.yml", "---\n- hosts: all\n", "the lab");
    git(&root, &["clone", "-q", "--bare", "seed", "fork.git"]);
    w.fork = root.join("fork.git");
    w.root = root;
}

#[given(
    expr = "upstream ansible-nas, whose default branch is {string}, with two commits the fork lacks"
)]
fn upstream(w: &mut Node, default: String) {
    let work = w.root.join("upstream-work");
    git(&w.root, &["clone", "-q", "seed", "upstream-work"]);
    git(&work, &["checkout", "-q", "-b", &default]);
    commit(
        &work,
        JOHN,
        "jellyfin.yml",
        "image: jellyfin:10\n",
        "add jellyfin",
    );
    commit(
        &work,
        JOHN,
        "sonarr.yml",
        "image: sonarr:4\n",
        "bump sonarr",
    );
    git(
        &w.root,
        &["init", "-q", "--bare", "-b", &default, "upstream.git"],
    );
    w.upstream = w.root.join("upstream.git");
    git(
        &work,
        &["push", "-q", w.upstream.to_str().unwrap(), &default],
    );
    w.upstream_work = work;
    let upstream = w.upstream.clone();
    w.keep(&upstream);
}

#[given("upstream cannot be reached")]
fn unreachable(w: &mut Node) {
    let nowhere = w.root.join("nowhere.git");
    w.keep(&nowhere);
}

#[when("a member checks out the fork")]
#[given("a member checked out the fork")]
fn checks_out(w: &mut Node) {
    w.check_out(0);
}

#[then(expr = "the checkout holds the fork's {string}")]
fn holds_the_fork(w: &mut Node, base: String) {
    let bundle = w.checkout.as_ref().expect("checked out");
    assert_eq!(bundle.branch, base);
    let repo = w.sandbox("sandbox");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), bundle.head);
}

#[then(expr = "it holds upstream's {string} as {string}")]
fn holds_upstream(w: &mut Node, branch: String, named: String) {
    let bundle = w.checkout.as_ref().expect("checked out");
    let upstream = bundle
        .upstream
        .as_ref()
        .expect("the checkout carries upstream");
    assert_eq!(upstream.branch, branch);
    assert_eq!(upstream.named(), named);
    let repo = w.sandbox("sandbox");
    assert_eq!(
        git(&repo, &["rev-parse", &named]),
        git(&w.upstream, &["rev-parse", &branch])
    );
}

#[then("it holds no upstream")]
fn no_upstream(w: &mut Node) {
    let bundle = w.checkout.as_ref().expect("checked out");
    assert_eq!(bundle.upstream, None);
}

#[then(expr = "offline, {string} lists the two commits upstream has")]
fn lists_upstreams_commits(w: &mut Node, command: String) {
    let repo = w.sandbox("sandbox");
    let args: Vec<&str> = command
        .strip_prefix("git ")
        .expect("a git command")
        .split_whitespace()
        .chain(["--format=%s"])
        .collect();
    assert_eq!(git(&repo, &args), "bump sonarr\nadd jellyfin");
}

#[when("upstream gains a commit")]
fn upstream_moves(w: &mut Node) {
    let work = w.upstream_work.clone();
    commit(&work, JOHN, "radarr.yml", "image: radarr:5\n", "add radarr");
    git(
        &work,
        &["push", "-q", w.upstream.to_str().unwrap(), "master"],
    );
}

fn upstream_head(bundle: &Bundle) -> String {
    bundle
        .upstream
        .as_ref()
        .expect("the checkout carries upstream")
        .head
        .clone()
}

#[then("a checkout within five minutes shows upstream as it was")]
fn not_yet(w: &mut Node) {
    let before = upstream_head(w.checkout.as_ref().expect("checked out"));
    let again = w.check_out(REFRESH_MS - 1);
    assert_eq!(upstream_head(&again), before);
}

#[then("a checkout five minutes later shows upstream's new commit")]
fn later(w: &mut Node) {
    let again = w.check_out(REFRESH_MS);
    assert_eq!(
        upstream_head(&again),
        git(&w.upstream, &["rev-parse", "master"])
    );
}

#[when(expr = "a member asks to check out {string}")]
fn asks_for(w: &mut Node, branch: String) {
    w.said = Some(w.mirror().bundle(Some(&branch), 0).map(|b| b.branch));
}

#[when(expr = "a member submits a branch named {string}")]
fn submits_named(w: &mut Node, branch: String) {
    w.said = Some(
        w.mirror()
            .submit(b"a bundle", &branch, 936, 0)
            .map(|t| t.title()),
    );
}

#[when(expr = "a member submits {string} with upstream's master merged into develop")]
fn submits_a_merge(w: &mut Node, branch: String) {
    w.check_out(0);
    let repo = w.sandbox("sandbox");
    git(&repo, &["checkout", "-q", "-b", &branch, "develop"]);
    let message = format!("take upstream\n\n{TRAILER}");
    let mut args: Vec<&str> = HQ.to_vec();
    args.extend(["merge", "-q", "--no-ff", "-m", &message, "upstream/master"]);
    git(&repo, &args);
    w.submit(&repo, &branch);
}

#[when(expr = "a member submits {string} with one file copied from upstream as HQ's own commit")]
fn submits_a_port(w: &mut Node, branch: String) {
    w.check_out(0);
    let repo = w.sandbox("sandbox");
    git(&repo, &["checkout", "-q", "-b", &branch, "develop"]);
    git(
        &repo,
        &["checkout", "upstream/master", "--", "jellyfin.yml"],
    );
    let message = format!("add jellyfin, as upstream has it\n\n{TRAILER}");
    let mut args: Vec<&str> = HQ.to_vec();
    args.extend(["commit", "-q", "-m", &message]);
    git(&repo, &args);
    w.submit(&repo, &branch);
}

#[given(expr = "a member's branch {string} was taken")]
fn was_taken(w: &mut Node, branch: String) {
    submits_a_port(w, branch);
    assert!(
        matches!(w.said, Some(Ok(_))),
        "the branch was not taken: {:?}",
        w.said
    );
}

#[when("the node is asked to push it to upstream")]
fn push_upstream(w: &mut Node) {
    let (branch, head) = w.taken.clone().expect("a branch was taken");
    let upstream = w.upstream.to_str().unwrap().to_owned();
    w.said = Some(w.mirror().push(&branch, &head, &upstream, None));
}

#[then(expr = "it is refused, saying {string}")]
fn refused_saying(w: &mut Node, words: String) {
    match &w.said {
        Some(Err(why)) => assert!(why.contains(&words), "refused otherwise: {why}"),
        other => panic!("not refused: {other:?}"),
    }
}

#[then("it is taken")]
fn taken(w: &mut Node) {
    assert!(matches!(w.said, Some(Ok(_))), "refused: {:?}", w.said);
}
