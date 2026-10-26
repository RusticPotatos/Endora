//! The lab's ansible-nas (ADR 0112): the owner's public fork of ansible-nas, which builds the
//! home lab (each app a role, its container made by the playbook). Endora reads two things of
//! it from GitHub, and nothing else: the role names (`roles/`), which say who manages a
//! container, and the docs (`docs/**/*.md`, `website/docs/**/*.md`, `roles/*/README.md`), which
//! the team reads through the `ansible_nas_doc` contract. Never an inventory, `group_vars`,
//! `host_vars`, a vault or any other file: [`asked`] is the allowlist every path passes, and the
//! listing is walked only inside the allowed folders.
//!
//! The fork is named by the node's environment (`ENDORA_ANSIBLE_NAS_REPO`, `owner/name`, and
//! `ENDORA_ANSIBLE_NAS_REF`, a branch; unset, the fork's own default branch, asked of GitHub
//! once per process and `main` when GitHub does not say, ADR 0115); with no repo, the docs are
//! not configured and every container's manager is unknown. What is read is kept for a day.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

/// How long the role list, the docs' listing and each doc are kept: a day.
pub const KEEP_FOR_MS: i64 = 24 * 60 * 60 * 1000;
/// How long a failed listing is kept before GitHub is asked again.
const FAILED_FOR_MS: i64 = 10 * 60 * 1000;
/// The most of one doc a turn is given.
pub const MOST_OF_A_DOC: usize = 16 * 1024;
/// The most folders the docs' listing walks.
const MOST_FOLDERS: usize = 64;

/// What a turn is told when no fork is configured.
pub const NOT_CONFIGURED: &str = "The lab's ansible-nas docs are not configured: the owner sets \
    ENDORA_ANSIBLE_NAS_REPO (owner/name of their ansible-nas fork on GitHub) for Endora's node. \
    Until then, who manages a container is unknown.";

/// What a turn is told of a path outside the allowlist.
pub const ONLY_THE_DOCS: &str = "only the lab's ansible-nas docs can be read: a file \
    docs/…/*.md, website/docs/…/*.md or roles/<name>/README.md, or the folder roles, docs or \
    website/docs (or a folder in those two) to list it";

/// The branch a fork is read at when GitHub does not say which is its default.
pub const FALLBACK_BRANCH: &str = "main";

/// The fork, as the environment names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fork {
    repo: String,
    /// The branch the owner named, or `None` for the fork's own default branch.
    reference: Option<String>,
}

impl Fork {
    /// The fork named `repo` (`owner/name`) at `reference` (empty: its default branch, which
    /// GitHub is asked for). `None` when no repo is given, or it is not a GitHub `owner/name`,
    /// or the reference is not a plain branch or tag name. The fork's GitHub address
    /// (`https://github.com/owner/name`) names it too.
    #[must_use]
    pub fn of(repo: &str, reference: &str) -> Option<Self> {
        let repo = on_github(repo.trim())?;
        let reference = match reference.trim() {
            "" => None,
            given => Some(given),
        };
        let plain = |s: &str| {
            !s.is_empty()
                && s.len() <= 100
                && !s.starts_with('.')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        };
        let (owner, name) = repo.split_once('/')?;
        if !plain(owner) || !plain(name) {
            return None;
        }
        if let Some(reference) = reference {
            if !plain_branch(reference) {
                return None;
            }
        }
        Some(Self {
            repo: repo.to_owned(),
            reference: reference.map(str::to_owned),
        })
    }

    /// `owner/name`.
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The branch the owner named, if they did.
    #[must_use]
    pub fn named_branch(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    /// Where git clones the fork from, unauthenticated: `https://github.com/owner/name.git`.
    #[must_use]
    pub fn https_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }

    /// Where `branch` opens on GitHub against the base the team works from, ready to become a
    /// pull request: `None` for a name that is not a plain branch.
    #[must_use]
    pub fn compare_url(&self, branch: &str) -> Option<String> {
        if !plain_branch(branch) {
            return None;
        }
        Some(match &self.reference {
            Some(base) => format!("https://github.com/{}/compare/{base}...{branch}", self.repo),
            None => format!("https://github.com/{}/compare/{branch}", self.repo),
        })
    }

    /// Where git pushes to the fork with a deploy key: `git@github.com:owner/name.git`.
    #[must_use]
    pub fn ssh_url(&self) -> String {
        format!("git@github.com:{}.git", self.repo)
    }

    /// From the node's environment: `ENDORA_ANSIBLE_NAS_REPO` and `ENDORA_ANSIBLE_NAS_REF`.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let repo = std::env::var("ENDORA_ANSIBLE_NAS_REPO").unwrap_or_default();
        let reference = std::env::var("ENDORA_ANSIBLE_NAS_REF").unwrap_or_default();
        if repo.trim().is_empty() {
            return None;
        }
        let fork = Self::of(&repo, &reference);
        if fork.is_none() {
            eprintln!(
                "ansible-nas: ENDORA_ANSIBLE_NAS_REPO or _REF is not a GitHub owner/name and branch; the docs are not configured"
            );
        }
        fork
    }
}

/// `owner/name` from the fork's GitHub address, or as given when it is not one. `None` for an
/// address somewhere other than GitHub.
fn on_github(repo: &str) -> Option<&str> {
    let unschemed = repo
        .strip_prefix("https://")
        .or_else(|| repo.strip_prefix("http://"));
    let host = unschemed.unwrap_or(repo);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path = match host.strip_prefix("github.com/") {
        Some(path) => path,
        None if unschemed.is_some() => return None,
        None => repo,
    };
    let path = path.strip_suffix('/').unwrap_or(path);
    Some(path.strip_suffix(".git").unwrap_or(path))
}

/// Whether `name` is a plain branch or tag name: segments of letters, digits, `-`, `_` and
/// `.`, none starting with a dot, no `..`, at most 100 characters.
#[must_use]
pub fn plain_branch(name: &str) -> bool {
    let plain = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    !name.is_empty()
        && name.len() <= 100
        && !name.contains("..")
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && name.split('/').all(plain)
}

/// What a path asks for, once it has passed the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// A folder to list: `""` (the top), `roles`, `docs`, `website/docs`, or a folder in the
    /// last two.
    Folder(String),
    /// A doc to read.
    File(String),
}

/// The allowlist: what a path may ask for, or why not. Only plain segments (letters, digits,
/// `.`, `_`, `-`; never `.` or `..` or one starting with a dot) are let through, never an
/// inventory, `group_vars`, `host_vars` or anything named for a vault, and then only the
/// docs: `docs/**/*.md`, `website/docs/**/*.md`, `roles/<name>/README.md`, and the folders
/// that hold them.
///
/// # Errors
/// [`ONLY_THE_DOCS`], for anything else.
pub fn asked(path: &str) -> Result<Asked, String> {
    let refused = || ONLY_THE_DOCS.to_owned();
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() {
        return Ok(Asked::Folder(String::new()));
    }
    if path.len() > 200 || path.starts_with('/') {
        return Err(refused());
    }
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |part: &&str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !parts.iter().all(plain) {
        return Err(refused());
    }
    let forbidden = |part: &&str| {
        let part = part.to_ascii_lowercase();
        matches!(
            part.as_str(),
            "inventories" | "inventory" | "group_vars" | "host_vars"
        ) || part.contains("vault")
    };
    if parts.iter().any(forbidden) {
        return Err(refused());
    }
    let under_docs = (parts[0] == "docs" && parts.len() >= 2)
        || (parts.len() >= 3 && parts[..2] == ["website", "docs"]);
    if path.ends_with(".md") {
        let a_role_readme = parts.len() == 3 && parts[0] == "roles" && parts[2] == "README.md";
        if a_role_readme || under_docs {
            return Ok(Asked::File(path.to_owned()));
        }
        return Err(refused());
    }
    let a_root = matches!(path, "roles" | "docs" | "website/docs");
    if a_root || (under_docs && !path.contains('.')) {
        return Ok(Asked::Folder(path.to_owned()));
    }
    Err(refused())
}

/// One GET over the node's network: the status and the body.
pub trait Fetch: Send + Sync {
    /// GETs `url`.
    ///
    /// # Errors
    /// A category (`connection`, `protocol`), never upstream text.
    fn get(&self, url: &str) -> Result<(u16, String), &'static str>;
}

/// The real fetch: ureq, with a timeout, few redirects and a bounded body.
pub struct Web;

impl Fetch for Web {
    fn get(&self, url: &str) -> Result<(u16, String), &'static str> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(3)
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|_| "connection")?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_to_string()
            .map_err(|_| "protocol")?;
        Ok((status, body))
    }
}

/// What the fork's listing holds: the role names, and every doc's path.
#[derive(Debug, Clone, Default)]
struct Listing {
    roles: Vec<String>,
    docs: Vec<String>,
}

/// The lab's ansible-nas docs: the fork, how GitHub is reached, and what was read of it.
pub struct Docs {
    fork: Option<Fork>,
    fetch: Box<dyn Fetch>,
    api: String,
    raw: String,
    listing: Mutex<Option<(i64, Result<Listing, &'static str>)>>,
    read: Mutex<HashMap<String, (i64, Option<String>)>>,
    /// The fork's default branch, once GitHub has said it (or failed to, when it was asked).
    default_branch: Mutex<Option<(i64, Option<String>)>>,
}

impl Docs {
    /// The docs of `fork` (none: not configured), read through `fetch` from GitHub.
    #[must_use]
    pub fn new(fork: Option<Fork>, fetch: Box<dyn Fetch>) -> Self {
        Self {
            fork,
            fetch,
            api: "https://api.github.com".to_owned(),
            raw: "https://raw.githubusercontent.com".to_owned(),
            listing: Mutex::new(None),
            read: Mutex::new(HashMap::new()),
            default_branch: Mutex::new(None),
        }
    }

    /// As the node's environment configures them, read over the web.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(Fork::from_env(), Box::new(Web))
    }

    /// The same, with GitHub's API and raw files at other origins (a test's fake).
    #[must_use]
    pub fn at(mut self, api: &str, raw: &str) -> Self {
        api.trim_end_matches('/').clone_into(&mut self.api);
        raw.trim_end_matches('/').clone_into(&mut self.raw);
        self
    }

    /// Whether a fork is configured.
    #[must_use]
    pub const fn configured(&self) -> bool {
        self.fork.is_some()
    }

    /// The fork's role names, or `None` when it is not configured or cannot be read.
    pub fn roles(&self, now: i64) -> Option<Vec<String>> {
        self.listing(now).ok().map(|l| l.roles)
    }

    /// The fork, as configured.
    #[must_use]
    pub fn fork(&self) -> Option<&Fork> {
        self.fork.as_ref()
    }

    /// The branch the fork is read at (ADR 0115): the one the owner named, else the fork's own
    /// default branch as GitHub says it (`GET /repos/{owner}/{name}`, `default_branch`), asked
    /// once per process and kept; when GitHub does not answer, [`FALLBACK_BRANCH`], and GitHub
    /// is asked again ten minutes on. `None` when no fork is configured.
    pub fn branch(&self, now: i64) -> Option<String> {
        let fork = self.fork.as_ref()?;
        if let Some(named) = &fork.reference {
            return Some(named.clone());
        }
        let mut kept = self
            .default_branch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match kept.as_ref() {
            Some((_, Some(branch))) => return Some(branch.clone()),
            Some((at, None)) if now - at < FAILED_FOR_MS => {
                return Some(FALLBACK_BRANCH.to_owned());
            }
            _ => {}
        }
        let found = self.ask_default_branch(fork);
        *kept = Some((now, found.clone()));
        Some(found.unwrap_or_else(|| FALLBACK_BRANCH.to_owned()))
    }

    /// GitHub's word on the fork's default branch, when it gives a plain one.
    fn ask_default_branch(&self, fork: &Fork) -> Option<String> {
        let url = format!("{}/repos/{}", self.api, fork.repo);
        let (status, body) = self.fetch.get(&url).ok()?;
        if status != 200 {
            return None;
        }
        let answer: Value = serde_json::from_str(&body).ok()?;
        let branch = answer["default_branch"].as_str()?.trim();
        plain_branch(branch).then(|| branch.to_owned())
    }

    /// One folder of the fork, as the contents API lists it: `None` when it is not there.
    fn folder(
        &self,
        fork: &Fork,
        branch: &str,
        folder: &str,
    ) -> Result<Option<Vec<Value>>, &'static str> {
        let reference = branch.replace('/', "%2F");
        let url = format!(
            "{}/repos/{}/contents/{folder}?ref={reference}",
            self.api, fork.repo
        );
        let (status, body) = self.fetch.get(&url)?;
        match status {
            200 => {}
            404 => return Ok(None),
            _ => return Err("upstream"),
        }
        let listed: Value = serde_json::from_str(&body).map_err(|_| "protocol")?;
        Ok(Some(listed.as_array().ok_or("protocol")?.clone()))
    }

    /// The role names and every doc's path, kept for a day (a failure, for ten minutes).
    fn listing(&self, now: i64) -> Result<Listing, &'static str> {
        let Some(fork) = &self.fork else {
            return Err("unconfigured");
        };
        let kept = self
            .listing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, listing)) = kept.as_ref() {
            let keep = if listing.is_ok() {
                KEEP_FOR_MS
            } else {
                FAILED_FOR_MS
            };
            if now - at < keep {
                return listing.clone();
            }
        }
        drop(kept);
        let branch = self.branch(now).ok_or("unconfigured")?;
        let listing = self.list(fork, &branch);
        *self
            .listing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((now, listing.clone()));
        listing
    }

    fn list(&self, fork: &Fork, branch: &str) -> Result<Listing, &'static str> {
        let roles: Vec<String> = self
            .folder(fork, branch, "roles")?
            .ok_or("no roles")?
            .iter()
            .filter(|e| e["type"] == "dir")
            .filter_map(|e| e["name"].as_str())
            .filter(|name| {
                matches!(
                    asked(&format!("roles/{name}/README.md")),
                    Ok(Asked::File(_))
                )
            })
            .map(str::to_owned)
            .collect();
        let mut docs = Vec::new();
        let mut folders = vec!["docs".to_owned(), "website/docs".to_owned()];
        let mut walked = 0;
        while let Some(folder) = folders.pop() {
            walked += 1;
            if walked > MOST_FOLDERS {
                break;
            }
            let Some(entries) = self.folder(fork, branch, &folder)? else {
                continue;
            };
            for entry in &entries {
                let path = entry["path"].as_str().unwrap_or("");
                // Only what is inside the folder asked, and passes the allowlist.
                if !path.starts_with(&format!("{folder}/")) {
                    continue;
                }
                match (entry["type"].as_str(), asked(path)) {
                    (Some("dir"), Ok(Asked::Folder(p))) => folders.push(p),
                    (Some("file"), Ok(Asked::File(p))) => docs.push(p),
                    _ => {}
                }
            }
        }
        docs.sort();
        Ok(Listing { roles, docs })
    }

    /// What the `ansible_nas_doc` contract answers for `path`: a folder's listing or a doc's
    /// text.
    ///
    /// # Errors
    /// What the turn is told: not configured, refused by the allowlist, not there, or GitHub
    /// not answering.
    pub fn read(&self, path: &str, now: i64) -> Result<String, String> {
        let Some(fork) = &self.fork else {
            return Err(NOT_CONFIGURED.to_owned());
        };
        let asked = asked(path)?;
        let listing = self.listing(now).map_err(|why| {
            format!("the lab's ansible-nas docs could not be read from GitHub ({why}); try later")
        })?;
        match asked {
            Asked::Folder(folder) => Ok(said_folder(&listing, &folder)),
            Asked::File(file) => self.file(fork, &listing, &file, now),
        }
    }

    fn file(&self, fork: &Fork, listing: &Listing, file: &str, now: i64) -> Result<String, String> {
        let branch = self
            .branch(now)
            .unwrap_or_else(|| FALLBACK_BRANCH.to_owned());
        let known = match file.strip_prefix("roles/") {
            Some(rest) => listing
                .roles
                .iter()
                .any(|r| rest.strip_suffix("/README.md") == Some(r.as_str())),
            None => listing.docs.iter().any(|d| d == file),
        };
        if !known {
            return Err(format!(
                "there is no {file} in the lab's ansible-nas: list its folder to see what is"
            ));
        }
        let mut read = self
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = match read.get(file) {
            Some((at, text)) if now - at < KEEP_FOR_MS => text.clone(),
            _ => {
                let url = format!("{}/{}/{branch}/{file}", self.raw, fork.repo);
                let text = match self.fetch.get(&url) {
                    Ok((200, body)) => Some(body),
                    Ok((404, _)) => None,
                    _ => {
                        return Err(format!("{file} could not be read from GitHub; try later"));
                    }
                };
                read.insert(file.to_owned(), (now, text.clone()));
                text
            }
        };
        let Some(text) = text else {
            return Err(format!("{file} is not there: that role has no README"));
        };
        Ok(format!(
            "{file}, from the lab's ansible-nas:\n{}",
            cut(&text)
        ))
    }
}

/// A doc held to [`MOST_OF_A_DOC`] bytes, on a character's edge, saying so when it is cut.
fn cut(text: &str) -> String {
    if text.len() <= MOST_OF_A_DOC {
        return text.to_owned();
    }
    let mut end = MOST_OF_A_DOC;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut: the rest is longer than 16 KB]", &text[..end])
}

/// A folder of the fork as a turn reads it: what is in it, folders first.
fn said_folder(listing: &Listing, folder: &str) -> String {
    if folder.is_empty() {
        return format!(
            "The lab's ansible-nas: roles ({} roles, each an app), docs and website/docs ({} docs). List a folder, or read a file such as roles/<name>/README.md or a doc under website/docs.",
            listing.roles.len(),
            listing.docs.len()
        );
    }
    if folder == "roles" {
        return format!(
            "The lab's ansible-nas roles ({}), each an app whose container the playbook makes: {}. A role's README, if it has one: roles/<name>/README.md; its app's page is under website/docs/applications.",
            listing.roles.len(),
            listing.roles.join(", ")
        );
    }
    let prefix = format!("{folder}/");
    let mut folders: Vec<String> = Vec::new();
    let mut files: Vec<&str> = Vec::new();
    for doc in listing.docs.iter().filter_map(|d| d.strip_prefix(&prefix)) {
        match doc.split_once('/') {
            Some((sub, _)) => {
                let sub = format!("{prefix}{sub}/");
                if !folders.contains(&sub) {
                    folders.push(sub);
                }
            }
            None => files.push(doc),
        }
    }
    if folders.is_empty() && files.is_empty() {
        return format!("{folder} holds no docs in the lab's ansible-nas.");
    }
    let files: Vec<String> = files.iter().map(|f| format!("{prefix}{f}")).collect();
    format!(
        "{folder}, in the lab's ansible-nas: {}",
        folders
            .into_iter()
            .chain(files)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const NOON: i64 = 1_792_238_400_000;

    /// GitHub, faked: answers by URL, and keeps every URL asked.
    #[derive(Default, Clone)]
    struct Hub {
        answers: Arc<HashMap<String, (u16, String)>>,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Fetch for Hub {
        fn get(&self, url: &str) -> Result<(u16, String), &'static str> {
            self.asked.lock().unwrap().push(url.to_owned());
            Ok(self
                .answers
                .get(url)
                .cloned()
                .unwrap_or((404, "{\"message\":\"Not Found\"}".to_owned())))
        }
    }

    const API: &str = "https://api.github.com/repos/owner/ansible-nas/contents";
    const RAW: &str = "https://raw.githubusercontent.com/owner/ansible-nas/main";

    fn entry(path: &str, kind: &str) -> Value {
        serde_json::json!({ "name": path.rsplit('/').next().unwrap(), "path": path, "type": kind })
    }

    fn hub() -> Hub {
        let listed = |entries: Vec<Value>| (200, Value::Array(entries).to_string());
        let mut answers = HashMap::new();
        // The fork's default branch is main, not master: GitHub says so.
        answers.insert(
            "https://api.github.com/repos/owner/ansible-nas".to_owned(),
            (
                200,
                r#"{"default_branch":"main","private":false}"#.to_owned(),
            ),
        );
        answers.insert(
            format!("{API}/roles?ref=main"),
            listed(vec![
                entry("roles/deluge", "dir"),
                entry("roles/gluetun", "dir"),
                entry("roles/nzbget", "dir"),
                entry("roles/.hidden", "dir"),
                entry("roles/notes.txt", "file"),
            ]),
        );
        answers.insert(
            format!("{API}/website/docs?ref=main"),
            listed(vec![
                entry("website/docs/applications", "dir"),
                entry("website/docs/index.md", "file"),
                entry("website/docs/logo.png", "file"),
                // A listing that strays outside its folder is not followed.
                entry("group_vars/all.yml", "file"),
                entry("inventories/x", "dir"),
            ]),
        );
        answers.insert(
            format!("{API}/website/docs/applications?ref=main"),
            listed(vec![entry(
                "website/docs/applications/download-tools",
                "dir",
            )]),
        );
        answers.insert(
            format!("{API}/website/docs/applications/download-tools?ref=main"),
            listed(vec![
                entry("website/docs/applications/download-tools/deluge.md", "file"),
                entry(
                    "website/docs/applications/download-tools/gluetun.md",
                    "file",
                ),
            ]),
        );
        answers.insert(
            format!("{RAW}/website/docs/applications/download-tools/gluetun.md"),
            (
                200,
                "# Gluetun\nOther containers can use its network.".to_owned(),
            ),
        );
        answers.insert(
            format!("{RAW}/roles/deluge/README.md"),
            (200, "# deluge\nA torrent client.".to_owned()),
        );
        Hub {
            answers: Arc::new(answers),
            asked: Arc::default(),
        }
    }

    fn docs(hub: &Hub) -> Docs {
        Docs::new(Fork::of("owner/ansible-nas", ""), Box::new(hub.clone()))
    }

    #[test]
    fn only_the_docs_pass_the_allowlist() {
        for allowed in [
            "roles/deluge/README.md",
            "docs/index.md",
            "website/docs/applications/download-tools/deluge.md",
        ] {
            assert_eq!(
                asked(allowed),
                Ok(Asked::File(allowed.to_owned())),
                "{allowed}"
            );
        }
        for folder in [
            "",
            "roles",
            "roles/",
            "docs",
            "website/docs",
            "website/docs/applications",
        ] {
            assert!(matches!(asked(folder), Ok(Asked::Folder(_))), "{folder}");
        }
        for refused in [
            "inventories/my-ansible-nas/inventory",
            "inventories/my-ansible-nas/group_vars/nas.yml",
            "group_vars/all.yml",
            "host_vars/nas.yml",
            "website/docs/group_vars/all.md",
            "docs/vault.md",
            "../group_vars/all.yml",
            "docs/../group_vars/all.yml",
            "website/docs/./index.md",
            "website/docs/%2e%2e/nas.yml",
            "/etc/passwd",
            "roles/deluge/tasks/main.yml",
            "roles/deluge/defaults/main.yml",
            "roles/deluge",
            "roles/deluge/docs/README.md",
            "website/docs/applications/deluge.yml",
            "website/README.md",
            "README.md",
            "nas.yml",
            "docs\\..\\nas.yml",
            "roles/.git/README.md",
        ] {
            assert_eq!(asked(refused), Err(ONLY_THE_DOCS.to_owned()), "{refused}");
        }
    }

    #[test]
    fn without_a_fork_the_docs_say_they_are_not_configured() {
        let hub = hub();
        let unset = Docs::new(None, Box::new(hub.clone()));
        assert!(!unset.configured());
        assert_eq!(unset.read("roles", NOON), Err(NOT_CONFIGURED.to_owned()));
        assert_eq!(unset.roles(NOON), None);
        assert!(hub.asked.lock().unwrap().is_empty(), "nothing fetched");
        assert_eq!(Fork::of("", "master"), None);
        assert_eq!(
            Fork::of("owner/ansible-nas", "").unwrap().named_branch(),
            None,
            "unset: the fork's own default"
        );
        assert_eq!(
            Fork::of("owner/ansible-nas", "develop")
                .unwrap()
                .named_branch(),
            Some("develop")
        );
        assert_eq!(
            Fork::of("owner/ansible-nas", "").unwrap().https_url(),
            "https://github.com/owner/ansible-nas.git"
        );
        assert_eq!(
            Fork::of("owner/ansible-nas", "").unwrap().ssh_url(),
            "git@github.com:owner/ansible-nas.git"
        );
        // A team branch on the owner's desk opens on GitHub, ready to become a pull request
        // against the base the team works from (owner, 2026-10-26).
        assert_eq!(
            Fork::of("owner/ansible-nas", "develop")
                .unwrap()
                .compare_url("hq/task-672-role-readmes"),
            Some(
                "https://github.com/owner/ansible-nas/compare/develop...hq/task-672-role-readmes"
                    .to_owned()
            )
        );
        assert_eq!(
            Fork::of("owner/ansible-nas", "")
                .unwrap()
                .compare_url("hq/task-1-x"),
            Some("https://github.com/owner/ansible-nas/compare/hq/task-1-x".to_owned()),
            "unset: against the fork's own default"
        );
        assert_eq!(
            Fork::of("owner/ansible-nas", "develop")
                .unwrap()
                .compare_url("../x?y"),
            None
        );
        assert_eq!(Fork::of("owner", ""), None, "owner/name");
        assert_eq!(Fork::of("owner/ansible-nas/../x", ""), None);
        assert_eq!(Fork::of("owner/ansible-nas", "../main"), None);
        assert!(Fork::of("owner/ansible-nas", "feature/x").is_some());
    }

    #[test]
    fn the_fork_may_be_named_by_its_github_address() {
        let named = Fork::of("owner/ansible-nas", "");
        for address in [
            "https://github.com/owner/ansible-nas",
            "https://github.com/owner/ansible-nas/",
            "https://github.com/owner/ansible-nas.git",
            "http://github.com/owner/ansible-nas",
            "github.com/owner/ansible-nas",
            "https://www.github.com/owner/ansible-nas",
        ] {
            assert_eq!(Fork::of(address, ""), named, "{address}");
        }
        for elsewhere in [
            "https://gitlab.com/owner/ansible-nas",
            "https://github.com.example.com/owner/ansible-nas",
            "https://github.com/owner/ansible-nas/tree/main",
            "https://github.com/owner",
        ] {
            assert_eq!(Fork::of(elsewhere, ""), None, "{elsewhere}");
        }
    }

    #[test]
    fn the_roles_and_the_docs_are_read_and_nothing_else_is_fetched() {
        let hub = hub();
        let docs = docs(&hub);
        assert_eq!(
            docs.roles(NOON),
            Some(vec![
                "deluge".to_owned(),
                "gluetun".to_owned(),
                "nzbget".to_owned()
            ])
        );
        let roles = docs.read("roles", NOON).unwrap();
        assert!(
            roles.contains("(3), each an app") && roles.contains("deluge, gluetun, nzbget"),
            "{roles}"
        );
        let folder = docs
            .read("website/docs/applications/download-tools", NOON)
            .unwrap();
        assert!(
            folder.contains("website/docs/applications/download-tools/gluetun.md"),
            "{folder}"
        );
        let top = docs.read("website/docs", NOON).unwrap();
        assert_eq!(
            top,
            "website/docs, in the lab's ansible-nas: website/docs/applications/, website/docs/index.md"
        );
        let page = docs
            .read("website/docs/applications/download-tools/gluetun.md", NOON)
            .unwrap();
        assert!(
            page.contains("Other containers can use its network."),
            "{page}"
        );
        assert!(
            docs.read("roles/deluge/README.md", NOON)
                .unwrap()
                .contains("A torrent client.")
        );
        assert!(
            docs.read("roles/gluetun/README.md", NOON)
                .unwrap_err()
                .contains("no README")
        );
        assert!(
            docs.read("roles/sonarr/README.md", NOON)
                .unwrap_err()
                .contains("there is no"),
            "an unknown role is not fetched"
        );
        assert!(docs.read("group_vars/all.yml", NOON).is_err());
        let asked = hub.asked.lock().unwrap().clone();
        for url in &asked {
            let path = url
                .strip_prefix(&format!("{API}/"))
                .map(|p| p.trim_end_matches("?ref=main"))
                .or_else(|| url.strip_prefix(&format!("{RAW}/")))
                .or_else(|| {
                    (url == "https://api.github.com/repos/owner/ansible-nas").then_some("roles")
                })
                .unwrap_or_else(|| panic!("{url} is neither the API nor raw"));
            assert!(
                path == "roles"
                    || path == "docs"
                    || path.starts_with("website/docs")
                    || (path.starts_with("roles/") && path.ends_with("/README.md")),
                "{url}"
            );
            assert!(
                !url.contains("group_vars") && !url.contains("inventor"),
                "{url}"
            );
        }
        // The listing and each doc are kept for a day.
        let before = asked.len();
        docs.read(
            "website/docs/applications/download-tools/gluetun.md",
            NOON + 60_000,
        )
        .unwrap();
        docs.read("roles/gluetun/README.md", NOON + 60_000)
            .unwrap_err();
        assert_eq!(
            hub.asked.lock().unwrap().len(),
            before,
            "nothing fetched again"
        );
        docs.read("roles", NOON + KEEP_FOR_MS).unwrap();
        assert!(
            hub.asked.lock().unwrap().len() > before,
            "a day on, it is read again"
        );
    }

    #[test]
    fn the_forks_default_branch_is_asked_of_github_once_and_main_when_it_does_not_say() {
        let hub = hub();
        let docs = docs(&hub);
        assert_eq!(docs.branch(NOON), Some("main".to_owned()));
        assert_eq!(docs.branch(NOON + 1), Some("main".to_owned()));
        docs.read("roles", NOON + 2).unwrap();
        let asked = hub.asked.lock().unwrap().clone();
        assert_eq!(
            asked
                .iter()
                .filter(|u| *u == "https://api.github.com/repos/owner/ansible-nas")
                .count(),
            1,
            "asked once per process: {asked:?}"
        );
        assert!(
            asked
                .iter()
                .any(|u| u.ends_with("/contents/roles?ref=main")),
            "the docs are read at that branch: {asked:?}"
        );
        // The owner named a branch: GitHub is not asked.
        let named = Docs::new(
            Fork::of("owner/ansible-nas", "develop"),
            Box::new(hub.clone()),
        );
        assert_eq!(named.branch(NOON), Some("develop".to_owned()));
        // GitHub does not say (a 404, an odd answer): main, and asked again later.
        let silent = Hub::default();
        let docs = Docs::new(Fork::of("owner/ansible-nas", ""), Box::new(silent.clone()));
        assert_eq!(docs.branch(NOON), Some("main".to_owned()));
        assert_eq!(docs.branch(NOON + 1), Some("main".to_owned()));
        assert_eq!(
            silent.asked.lock().unwrap().len(),
            1,
            "not asked again at once"
        );
        assert_eq!(docs.branch(NOON + FAILED_FOR_MS).as_deref(), Some("main"));
        assert_eq!(
            silent.asked.lock().unwrap().len(),
            2,
            "asked again ten minutes on"
        );
        let mut odd = HashMap::new();
        odd.insert(
            "https://api.github.com/repos/owner/ansible-nas".to_owned(),
            (200, r#"{"default_branch":"../etc"}"#.to_owned()),
        );
        let odd = Docs::new(
            Fork::of("owner/ansible-nas", ""),
            Box::new(Hub {
                answers: Arc::new(odd),
                asked: Arc::default(),
            }),
        );
        assert_eq!(
            odd.branch(NOON).as_deref(),
            Some("main"),
            "not a plain name"
        );
        assert_eq!(Docs::new(None, Box::new(hub)).branch(NOON), None);
        assert!(plain_branch("main") && plain_branch("feature/x-1.2"));
        for bad in [
            "", "../x", "/main", "main/", ".hidden", "a..b", "x.lock", "a b",
        ] {
            assert!(!plain_branch(bad), "{bad}");
        }
    }

    #[test]
    fn a_long_doc_is_cut_on_a_characters_edge() {
        let long = "é".repeat(MOST_OF_A_DOC);
        let cut = cut(&long);
        assert!(cut.ends_with("[cut: the rest is longer than 16 KB]"));
        assert!(cut.len() <= MOST_OF_A_DOC + 60);
    }

    #[test]
    fn the_web_fetch_reads_over_http_with_its_timeout() {
        use std::io::{BufRead as _, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            // The whole request first (a GET: its head, to the empty line), then the answer.
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                head.push(line);
            }
            let body = "[]";
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            head
        });
        let got = Web
            .get(&format!(
                "http://127.0.0.1:{port}/repos/owner/ansible-nas/contents/roles?ref=main"
            ))
            .unwrap();
        assert_eq!(got, (200, "[]".to_owned()));
        let head = server.join().unwrap();
        assert!(
            head[0].starts_with("GET /repos/owner/ansible-nas/contents/roles?ref=main "),
            "{head:?}"
        );
        assert!(head.iter().any(|h| {
            h.to_ascii_lowercase()
                .starts_with("accept: application/vnd.github+json")
        }));
    }
}
