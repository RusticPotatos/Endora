//! Which tags an image has on its registry, for the team's `image_tags` contract (ADR 0106,
//! amended 2026-10-30). A sandbox is offline, so a member pinning an image off `latest` could
//! not confirm the release it chose exists; the node asks the registry for it.
//!
//! Only the public registries a lab's images come from are asked, anonymously: Docker Hub
//! (its own API), and ghcr.io and quay.io by the OCI distribution API with an anonymous pull
//! token. lscr.io serves linuxserver's images from ghcr.io, so it is read there. The reference
//! is checked strictly and every URL is built here from its checked parts; nothing a registry
//! answers is followed (no redirect, no `next` link), and no credential is ever sent.

use serde_json::{Value, json};
use std::time::Duration;

/// The most tags a lookup gives back.
pub const MOST_TAGS: usize = 30;
/// The most of one registry answer that is read.
pub const MOST_OF_AN_ANSWER: u64 = 2 * 1024 * 1024;
/// How long one request to a registry may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The most pages of Docker Hub's tags read (100 a page, newest first).
const HUB_PAGES: usize = 3;
/// The most pages of an OCI registry's tag list read.
const OCI_PAGES: usize = 10;

/// What the team is told for an image on any other registry.
pub const ONLY_THESE: &str = "only images on Docker Hub (docker.io), ghcr.io, quay.io and lscr.io \
    can be looked up, by a reference like grafana/grafana, ghcr.io/gethomepage/homepage or \
    lscr.io/linuxserver/sonarr:4.0.0";

/// The only addresses a lookup ever asks: each registry's API.
const BASES: [&str; 3] = [
    "https://hub.docker.com/v2/",
    "https://ghcr.io/",
    "https://quay.io/",
];

/// Whether `url` is one a lookup may ask.
#[must_use]
pub fn allowed(url: &str) -> bool {
    BASES.iter().any(|base| url.starts_with(base))
}

/// The registries asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registry {
    /// Docker Hub, through `hub.docker.com`'s API.
    DockerHub,
    /// GitHub's container registry (and lscr.io, which serves from it).
    Ghcr,
    /// Red Hat's quay.io.
    Quay,
}

impl Registry {
    /// Its host, as an image reference names it.
    #[must_use]
    pub const fn host(self) -> &'static str {
        match self {
            Self::DockerHub => "docker.io",
            Self::Ghcr => "ghcr.io",
            Self::Quay => "quay.io",
        }
    }
}

/// An image reference, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Where its tags are read.
    pub registry: Registry,
    /// The host the reference named, when it differs from where it is read (lscr.io).
    pub via: Option<&'static str>,
    /// Its repository: `library/telegraf`, `gethomepage/homepage`.
    pub repository: String,
    /// The tag the reference named, if any.
    pub tag: Option<String>,
}

impl Image {
    /// The image as the team reads it: `docker.io/grafana/grafana`, `lscr.io/linuxserver/sonarr`.
    #[must_use]
    pub fn shown(&self) -> String {
        format!(
            "{}/{}",
            self.via.unwrap_or(self.registry.host()),
            self.repository
        )
    }
}

fn malformed() -> String {
    "that is not an image reference: name an image like grafana/grafana, \
     ghcr.io/gethomepage/homepage or lscr.io/linuxserver/sonarr, with :tag if you like"
        .to_owned()
}

/// Whether `part` is one path component of a repository name: lowercase letters, digits and
/// `.`, `_`, `-`, beginning and ending with a letter or digit.
fn component(part: &str) -> bool {
    let bytes = part.as_bytes();
    let edge = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    (1..=128).contains(&part.len())
        && edge(bytes.first())
        && edge(bytes.last())
        && !part.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(b))
}

/// Whether `tag` is a tag as Docker writes one: a letter, digit or `_`, then up to 127 more of
/// those, `.` or `-`.
#[must_use]
pub fn valid_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    (1..=128).contains(&tag.len())
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(b))
}

/// The image `reference` names, or why it cannot be looked up: [`ONLY_THESE`] for another
/// registry, and a plain refusal for anything that is not a reference (a URL, a digest, a
/// port, upper case, spaces).
///
/// # Errors
/// What the model reads.
pub fn parse(reference: &str) -> Result<Image, String> {
    let reference = reference.trim();
    if reference.is_empty()
        || reference.len() > 255
        || reference.contains("://")
        || reference.contains('@')
        || reference
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(malformed());
    }
    let (host, path) = match reference.split_once('/') {
        Some((first, rest))
            if first.contains('.') || first.contains(':') || first == "localhost" =>
        {
            (first.to_ascii_lowercase(), rest)
        }
        _ => ("docker.io".to_owned(), reference),
    };
    let (registry, via) = match host.as_str() {
        "docker.io" | "index.docker.io" | "registry-1.docker.io" | "registry.hub.docker.com" => {
            (Registry::DockerHub, None)
        }
        "ghcr.io" => (Registry::Ghcr, None),
        "lscr.io" => (Registry::Ghcr, Some("lscr.io")),
        "quay.io" => (Registry::Quay, None),
        _ => return Err(ONLY_THESE.to_owned()),
    };
    let (path, tag) = match path.rsplit_once(':') {
        Some((path, tag)) => (path, Some(tag)),
        None => (path, None),
    };
    if tag.is_some_and(|t| !valid_tag(t)) {
        return Err(malformed());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 4 || !parts.iter().all(|p| component(p)) {
        return Err(malformed());
    }
    let repository = match (registry, parts.len()) {
        (Registry::DockerHub, 1) => format!("library/{path}"),
        (Registry::DockerHub, 2) => path.to_owned(),
        (Registry::DockerHub, _) | (_, 1) => return Err(malformed()),
        _ => path.to_owned(),
    };
    Ok(Image {
        registry,
        via,
        repository,
        tag: tag.map(str::to_owned),
    })
}

/// One lookup, as the contract is called: the image, a tag to check, and the filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The image.
    pub image: Image,
    /// The tag whose existence is asked: the `tag` argument, else the reference's own.
    pub tag: Option<String>,
    /// Only tags starting with this.
    pub prefix: Option<String>,
    /// Only plain releases, `1.2.3` or `v1.2.3`.
    pub semver_only: bool,
}

/// The lookup the contract's arguments ask for (`image`, and optionally `tag`, `prefix`,
/// `semver_only`).
///
/// # Errors
/// What the model reads: [`ONLY_THESE`], or what is wrong with an argument.
pub fn asked_of(args: &Value) -> Result<Asked, String> {
    let Some(reference) = args["image"].as_str() else {
        return Err(
            "name the image (image), e.g. grafana/grafana or ghcr.io/gethomepage/homepage"
                .to_owned(),
        );
    };
    let image = parse(reference)?;
    let text = |key: &str| -> Result<Option<String>, String> {
        match &args[key] {
            Value::Null => Ok(None),
            Value::String(s) if s.trim().is_empty() => Ok(None),
            Value::String(s) if valid_tag(s.trim()) => Ok(Some(s.trim().to_owned())),
            _ => Err(format!(
                "{key} is a tag's text: letters, digits, '.', '_' and '-', e.g. 1.2.3 or v1."
            )),
        }
    };
    let tag = text("tag")?.or_else(|| image.tag.clone());
    let prefix = text("prefix")?;
    let semver_only = match &args["semver_only"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => return Err("semver_only is true or false".to_owned()),
    };
    Ok(Asked {
        image,
        tag,
        prefix,
        semver_only,
    })
}

/// One GET to a registry, anonymous unless a registry's own pull token is given.
pub trait Fetch: Send + Sync {
    /// GETs `url`, with `bearer` as its token.
    ///
    /// # Errors
    /// A category (`refused`, `connection`, `protocol`), never upstream text.
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<(u16, String), &'static str>;
}

/// The real fetch: ureq, HTTPS to the registries alone, no redirect, a timeout and a bounded
/// answer.
pub struct Web;

impl Fetch for Web {
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<(u16, String), &'static str> {
        if !allowed(url) {
            return Err("refused");
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(TIMEOUT))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.get(url).header("Accept", "application/json");
        if let Some(token) = bearer {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let mut response = request.call().map_err(|_| "connection")?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MOST_OF_AN_ANSWER)
            .read_to_string()
            .map_err(|_| "protocol")?;
        Ok((status, body))
    }
}

/// Why a lookup gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// The registry has no such public image.
    NotFound(String),
    /// The registry could not be read.
    Unreachable(String),
}

impl Failed {
    /// What the model reads.
    #[must_use]
    pub fn said(&self) -> &str {
        match self {
            Self::NotFound(s) | Self::Unreachable(s) => s,
        }
    }
}

/// What the registry said, before it is filtered.
struct Read {
    /// Every tag read, in the registry's order.
    tags: Vec<String>,
    /// Whether every tag was read.
    all: bool,
    /// Whether the asked tag exists, when the registry was asked it directly.
    exists: Option<bool>,
}

fn unread(image: &Image, status: u16) -> Failed {
    let host = image.registry.host();
    if status == 429 {
        return Failed::Unreachable(format!(
            "{host} is limiting requests just now: try again later"
        ));
    }
    Failed::Unreachable(format!("{host} answered {status}"))
}

fn not_found(image: &Image) -> Failed {
    Failed::NotFound(format!(
        "{} has no public image {}",
        image.via.unwrap_or(image.registry.host()),
        image.shown()
    ))
}

fn get(
    fetch: &dyn Fetch,
    url: &str,
    bearer: Option<&str>,
    image: &Image,
) -> Result<(u16, String), Failed> {
    fetch.get(url, bearer).map_err(|why| {
        Failed::Unreachable(format!(
            "{} could not be read ({why})",
            image.registry.host()
        ))
    })
}

/// Docker Hub: the newest tags by when each was pushed, a page of 100 at a time (only those
/// containing the prefix, when there is one), and the asked tag by its own address.
fn docker_hub(fetch: &dyn Fetch, asked: &Asked) -> Result<Read, Failed> {
    let image = &asked.image;
    let base = format!(
        "https://hub.docker.com/v2/repositories/{}/tags",
        image.repository
    );
    let mut tags = Vec::new();
    let mut all = false;
    for page in 1..=HUB_PAGES {
        let mut url = format!("{base}?page_size=100&ordering=last_updated&page={page}");
        if let Some(prefix) = &asked.prefix {
            url.push_str(&format!("&name={prefix}"));
        }
        let (status, body) = get(fetch, &url, None, image)?;
        match status {
            200 => {}
            404 if page == 1 => return Err(not_found(image)),
            _ => return Err(unread(image, status)),
        }
        let answer: Value = serde_json::from_str(&body).map_err(|_| unread(image, status))?;
        tags.extend(
            answer["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["name"].as_str())
                .map(str::to_owned),
        );
        if answer["next"].is_null() {
            all = true;
            break;
        }
    }
    let mut exists = None;
    if let Some(tag) = &asked.tag {
        if tags.contains(tag) {
            exists = Some(true);
        } else {
            let (status, _) = get(fetch, &format!("{base}/{tag}"), None, image)?;
            exists = match status {
                200 => Some(true),
                404 => Some(false),
                _ => return Err(unread(image, status)),
            };
        }
    }
    Ok(Read { tags, all, exists })
}

/// An OCI registry: an anonymous pull token, then the tag list a page at a time.
fn oci(fetch: &dyn Fetch, image: &Image) -> Result<Read, Failed> {
    let repository = &image.repository;
    let (token_url, page_size) = match image.registry {
        Registry::Quay => (
            format!("https://quay.io/v2/auth?service=quay.io&scope=repository:{repository}:pull"),
            100,
        ),
        _ => (
            format!("https://ghcr.io/token?service=ghcr.io&scope=repository:{repository}:pull"),
            1000,
        ),
    };
    let (status, body) = get(fetch, &token_url, None, image)?;
    match status {
        200 => {}
        401 | 403 | 404 => return Err(not_found(image)),
        _ => return Err(unread(image, status)),
    }
    let token: Value = serde_json::from_str(&body).map_err(|_| unread(image, status))?;
    let token = token["token"]
        .as_str()
        .or_else(|| token["access_token"].as_str())
        .ok_or_else(|| unread(image, status))?
        .to_owned();
    let host = image.registry.host();
    let mut tags: Vec<String> = Vec::new();
    let mut all = false;
    for page in 0..OCI_PAGES {
        let mut url = format!("https://{host}/v2/{repository}/tags/list?n={page_size}");
        if page > 0 {
            // Only a tag of a checked shape goes back into an address.
            let Some(last) = tags.last().filter(|t| valid_tag(t)) else {
                break;
            };
            url.push_str(&format!("&last={last}"));
        }
        let (status, body) = get(fetch, &url, Some(&token), image)?;
        match status {
            200 => {}
            401 | 403 | 404 if page == 0 => return Err(not_found(image)),
            _ => return Err(unread(image, status)),
        }
        let answer: Value = serde_json::from_str(&body).map_err(|_| unread(image, status))?;
        let listed: Vec<String> = answer["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let short = listed.len() < page_size;
        tags.extend(listed);
        if short {
            all = true;
            break;
        }
    }
    Ok(Read {
        tags,
        all,
        exists: None,
    })
}

/// A version's parts and whether anything follows them (`12.4.14-ubuntu`), for a tag that
/// begins with a version of at least two parts (`v1.2`, `1.40.2`); `None` for anything else
/// (`latest`, `main`, a date).
fn version_of(tag: &str) -> Option<(Vec<u64>, bool)> {
    let rest = tag.strip_prefix('v').unwrap_or(tag);
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(rest.len());
    let numbers = rest[..end].trim_end_matches('.');
    let parts: Option<Vec<u64>> = numbers.split('.').map(|p| p.parse().ok()).collect();
    let parts = parts.filter(|p| p.len() >= 2)?;
    Some((parts, end < rest.len()))
}

/// Whether `tag` is a plain release: `1.2.3` or `v1.2.3`, nothing after.
#[must_use]
pub fn is_release(tag: &str) -> bool {
    version_of(tag).is_some_and(|(parts, suffixed)| parts.len() == 3 && !suffixed)
        && !tag.ends_with('.')
}

/// Newest first: versions highest first, a plain one before its suffixed builds; then every
/// other tag in the registry's order.
fn newest_first(tags: &mut [String]) {
    let key = |tag: &String| version_of(tag);
    tags.sort_by(|a, b| match (key(a), key(b)) {
        (Some((va, sa)), Some((vb, sb))) => vb.cmp(&va).then(sa.cmp(&sb)).then(a.cmp(b)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// Looks `asked` up through `fetch`: the newest [`MOST_TAGS`] tags that pass its filters,
/// how many were read and whether that was all of them, and whether its tag exists.
///
/// # Errors
/// [`Failed`] when the image is not there or the registry cannot be read.
pub fn look_up(fetch: &dyn Fetch, asked: &Asked) -> Result<Value, Failed> {
    let image = &asked.image;
    let read = match image.registry {
        Registry::DockerHub => docker_hub(fetch, asked)?,
        Registry::Ghcr | Registry::Quay => oci(fetch, image)?,
    };
    let mut tags: Vec<String> = read.tags.into_iter().filter(|t| valid_tag(t)).collect();
    let count = tags.len();
    let exists = read.exists.or_else(|| {
        let tag = asked.tag.as_ref()?;
        if tags.contains(tag) {
            return Some(true);
        }
        read.all.then_some(false)
    });
    tags.retain(|t| {
        asked
            .prefix
            .as_ref()
            .is_none_or(|p| t.starts_with(p.as_str()))
            && (!asked.semver_only || is_release(t))
    });
    let matching = tags.len();
    newest_first(&mut tags);
    tags.truncate(MOST_TAGS);
    let mut answer = json!({
        "image": image.shown(),
        "tags_read": count,
        "all_read": read.all,
        "matching": matching,
        "newest": tags,
    });
    if let Some(via) = image.via {
        answer["note"] = format!(
            "{via} serves these images from {}: read there",
            image.registry.host()
        )
        .into();
    }
    if let Some(tag) = &asked.tag {
        answer["tag"] = tag.clone().into();
        answer["exists"] = exists.map_or(Value::Null, Value::Bool);
        if exists.is_none() {
            answer["exists_note"] =
                "not among the tags read, and not every tag was read: unknown".into();
        }
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A URL asked, and the token it was asked with.
    type AskedOf = (String, Option<String>);

    /// The registries, faked: answers by URL, and keeps every URL asked with its token.
    #[derive(Default, Clone)]
    struct Registries {
        answers: Arc<HashMap<String, (u16, String)>>,
        asked: Arc<Mutex<Vec<AskedOf>>>,
    }

    impl Fetch for Registries {
        fn get(&self, url: &str, bearer: Option<&str>) -> Result<(u16, String), &'static str> {
            self.asked
                .lock()
                .unwrap()
                .push((url.to_owned(), bearer.map(str::to_owned)));
            Ok(self
                .answers
                .get(url)
                .cloned()
                .unwrap_or((404, "{}".to_owned())))
        }
    }

    impl Registries {
        fn asked(&self) -> Vec<(String, Option<String>)> {
            self.asked.lock().unwrap().clone()
        }
    }

    fn registries() -> Registries {
        let mut answers = HashMap::new();
        let hub = "https://hub.docker.com/v2/repositories";
        answers.insert(
            format!("{hub}/grafana/grafana/tags?page_size=100&ordering=last_updated&page=1"),
            (
                200,
                json!({ "next": format!("{hub}/grafana/grafana/tags?page=2"), "results": [
                    { "name": "latest" }, { "name": "12.4.14" }, { "name": "12.4" },
                    { "name": "12.4.14-ubuntu" }, { "name": "main-ubuntu" },
                ] })
                .to_string(),
            ),
        );
        answers.insert(
            format!("{hub}/grafana/grafana/tags?page_size=100&ordering=last_updated&page=2"),
            (
                200,
                json!({ "next": null, "results": [ { "name": "12.3.9" }, { "name": "9.5.21" } ] })
                    .to_string(),
            ),
        );
        answers.insert(
            format!("{hub}/library/telegraf/tags?page_size=100&ordering=last_updated&page=1&name=1.40"),
            (
                200,
                json!({ "next": null, "results": [ { "name": "1.40.2" }, { "name": "1.40.2-alpine" }, { "name": "1.40" } ] })
                    .to_string(),
            ),
        );
        answers.insert(
            format!("{hub}/library/telegraf/tags/1.39.0"),
            (200, "{}".to_owned()),
        );
        answers.insert(
            "https://ghcr.io/token?service=ghcr.io&scope=repository:gethomepage/homepage:pull"
                .to_owned(),
            (200, json!({ "token": "anon" }).to_string()),
        );
        answers.insert(
            "https://ghcr.io/v2/gethomepage/homepage/tags/list?n=1000".to_owned(),
            (
                200,
                json!({ "name": "gethomepage/homepage", "tags": ["main", "v0.9.9", "latest", "v1.4.5", "v1.10.0", "v1.4.5-rc1", "nightly"] })
                    .to_string(),
            ),
        );
        answers.insert(
            "https://ghcr.io/token?service=ghcr.io&scope=repository:linuxserver/sonarr:pull"
                .to_owned(),
            (200, json!({ "token": "anon2" }).to_string()),
        );
        // A full first page sends the lookup on for a second.
        let full: Vec<String> = (0..1000).map(|i| format!("4.0.{i}")).collect();
        answers.insert(
            "https://ghcr.io/v2/linuxserver/sonarr/tags/list?n=1000".to_owned(),
            (200, json!({ "tags": full }).to_string()),
        );
        answers.insert(
            "https://ghcr.io/v2/linuxserver/sonarr/tags/list?n=1000&last=4.0.999".to_owned(),
            (200, json!({ "tags": ["4.1.0", "develop"] }).to_string()),
        );
        answers.insert(
            "https://quay.io/v2/auth?service=quay.io&scope=repository:prometheus/node-exporter:pull"
                .to_owned(),
            (200, json!({ "token": "q" }).to_string()),
        );
        answers.insert(
            "https://quay.io/v2/prometheus/node-exporter/tags/list?n=100".to_owned(),
            (
                200,
                json!({ "tags": ["v1.8.2", "v1.9.1", "master", "latest"] }).to_string(),
            ),
        );
        Registries {
            answers: Arc::new(answers),
            asked: Arc::default(),
        }
    }

    fn asked(args: Value) -> Asked {
        asked_of(&args).unwrap()
    }

    #[test]
    fn a_reference_is_read_strictly_and_only_for_the_four_registries() {
        let image = parse("telegraf").unwrap();
        assert_eq!(image.registry, Registry::DockerHub);
        assert_eq!(image.repository, "library/telegraf");
        assert_eq!(image.shown(), "docker.io/library/telegraf");
        let image = parse("grafana/grafana:12.0.0").unwrap();
        assert_eq!(
            (image.repository.as_str(), image.tag.as_deref()),
            ("grafana/grafana", Some("12.0.0"))
        );
        assert_eq!(
            parse("docker.io/prom/prometheus").unwrap().repository,
            "prom/prometheus"
        );
        let image = parse("ghcr.io/gethomepage/homepage:latest").unwrap();
        assert_eq!(image.registry, Registry::Ghcr);
        assert_eq!(image.repository, "gethomepage/homepage");
        let image = parse("lscr.io/linuxserver/sonarr").unwrap();
        assert_eq!(
            (image.registry, image.via),
            (Registry::Ghcr, Some("lscr.io"))
        );
        assert_eq!(image.shown(), "lscr.io/linuxserver/sonarr");
        assert_eq!(
            parse("quay.io/prometheus/node-exporter").unwrap().registry,
            Registry::Quay
        );
        assert_eq!(
            parse("miguelndecarvalho/speedtest-exporter")
                .unwrap()
                .repository,
            "miguelndecarvalho/speedtest-exporter"
        );
        // Any other host is refused, as is anything that is not a reference.
        for elsewhere in [
            "registry.example.com/team/app",
            "localhost/app",
            "127.0.0.1/a/b",
            "192.168.1.10:5000/app",
            "gcr.io/distroless/static",
        ] {
            assert_eq!(parse(elsewhere), Err(ONLY_THESE.to_owned()), "{elsewhere}");
        }
        for malformed in [
            "",
            "https://ghcr.io/gethomepage/homepage",
            "grafana/grafana@sha256:abc",
            "Grafana/Grafana",
            "grafana/../grafana",
            "grafana//grafana",
            "a/b/c",
            "ghcr.io/homepage",
            "grafana/grafana:",
            "grafana/grafana:-bad",
            "grafana/grafana: 1.0",
            "grafana grafana",
            "grafana/grafana?x=1",
            "grafana/grafana#x",
            "ghcr.io/a/b/c/d/e",
        ] {
            let refused = parse(malformed);
            assert!(refused.is_err(), "{malformed}");
            assert_ne!(refused, Err(ONLY_THESE.to_owned()), "{malformed}");
        }
    }

    #[test]
    fn the_arguments_are_checked_too() {
        let a = asked(json!({ "image": "grafana/grafana:12.0.0" }));
        assert_eq!(a.tag.as_deref(), Some("12.0.0"), "the reference's tag");
        let a = asked(
            json!({ "image": "grafana/grafana:12.0.0", "tag": "12.1.0", "prefix": "12.", "semver_only": true }),
        );
        assert_eq!(
            (a.tag.as_deref(), a.prefix.as_deref(), a.semver_only),
            (Some("12.1.0"), Some("12."), true)
        );
        assert!(asked_of(&json!({})).is_err());
        assert!(asked_of(&json!({ "image": 3 })).is_err());
        assert!(asked_of(&json!({ "image": "telegraf", "prefix": "1&name=x" })).is_err());
        assert!(asked_of(&json!({ "image": "telegraf", "tag": "a/b" })).is_err());
        assert!(asked_of(&json!({ "image": "telegraf", "semver_only": "yes" })).is_err());
    }

    #[test]
    fn only_the_registries_apis_are_ever_asked() {
        assert!(allowed(
            "https://hub.docker.com/v2/repositories/library/x/tags"
        ));
        assert!(allowed("https://ghcr.io/v2/a/b/tags/list"));
        assert!(allowed("https://quay.io/v2/a/b/tags/list"));
        for not in [
            "http://ghcr.io/v2/a/b/tags/list",
            "https://ghcr.io.example.com/v2/",
            "https://example.com/https://ghcr.io/",
            "https://hub.docker.com/u/x",
            "http://127.0.0.1:9/",
        ] {
            assert!(!allowed(not), "{not}");
            assert_eq!(Web.get(not, None), Err("refused"), "{not}");
        }
    }

    #[test]
    fn docker_hub_gives_the_newest_versions_first_and_says_whether_a_tag_exists() {
        let hub = registries();
        let answer = look_up(
            &hub,
            &asked(json!({ "image": "grafana/grafana", "tag": "12.3.9" })),
        )
        .unwrap();
        assert_eq!(answer["image"], "docker.io/grafana/grafana");
        assert_eq!(
            answer["newest"],
            json!([
                "12.4.14",
                "12.4.14-ubuntu",
                "12.4",
                "12.3.9",
                "9.5.21",
                "latest",
                "main-ubuntu"
            ])
        );
        assert_eq!(
            (answer["tags_read"].as_u64(), answer["all_read"].as_bool()),
            (Some(7), Some(true))
        );
        assert_eq!(answer["exists"], true);
        // Releases alone.
        let answer = look_up(
            &hub,
            &asked(json!({ "image": "grafana/grafana", "semver_only": true })),
        )
        .unwrap();
        assert_eq!(answer["newest"], json!(["12.4.14", "12.3.9", "9.5.21"]));
        assert_eq!(answer["matching"], 3);
        // A prefix narrows what Hub is asked, and a tag not among those read is asked itself.
        let answer = look_up(
            &hub,
            &asked(json!({ "image": "telegraf", "prefix": "1.40", "tag": "1.39.0" })),
        )
        .unwrap();
        assert_eq!(answer["newest"], json!(["1.40.2", "1.40.2-alpine", "1.40"]));
        assert_eq!(answer["exists"], true);
        let answer = look_up(
            &hub,
            &asked(json!({ "image": "telegraf:9.9.9", "prefix": "1.40" })),
        )
        .unwrap();
        assert_eq!(answer["exists"], false);
        // Nothing went anywhere but Hub's API, and no token was sent.
        assert!(
            hub.asked().iter().all(
                |(url, token)| url.starts_with("https://hub.docker.com/v2/") && token.is_none()
            )
        );
        // An image Hub does not have.
        let gone = look_up(&hub, &asked(json!({ "image": "nobody/nothing" }))).unwrap_err();
        assert!(matches!(gone, Failed::NotFound(_)), "{gone:?}");
        assert!(gone.said().contains("docker.io/nobody/nothing"), "{gone:?}");
    }

    #[test]
    fn ghcr_quay_and_lscr_are_read_with_an_anonymous_token_page_by_page() {
        let registries = registries();
        let answer = look_up(
            &registries,
            &asked(json!({ "image": "ghcr.io/gethomepage/homepage:v1.4.5" })),
        )
        .unwrap();
        assert_eq!(
            answer["newest"],
            json!([
                "v1.10.0",
                "v1.4.5",
                "v1.4.5-rc1",
                "v0.9.9",
                "main",
                "latest",
                "nightly"
            ])
        );
        assert_eq!(answer["exists"], true);
        let asked_ghcr = registries.asked();
        assert_eq!(asked_ghcr[0].1, None, "the token is asked anonymously");
        assert_eq!(asked_ghcr[1].1.as_deref(), Some("anon"));
        let answer = look_up(
            &registries,
            &asked(json!({ "image": "ghcr.io/gethomepage/homepage", "tag": "v9.9.9", "semver_only": true })),
        )
        .unwrap();
        assert_eq!(answer["newest"], json!(["v1.10.0", "v1.4.5", "v0.9.9"]));
        assert_eq!(answer["exists"], false, "every tag was read");
        // lscr.io is read on ghcr.io, a full page leading to the next.
        let answer = look_up(
            &registries,
            &asked(json!({ "image": "lscr.io/linuxserver/sonarr", "tag": "4.1.0" })),
        )
        .unwrap();
        assert_eq!(answer["image"], "lscr.io/linuxserver/sonarr");
        assert_eq!(answer["tags_read"], 1002);
        assert_eq!(answer["newest"][0], "4.1.0");
        assert_eq!(answer["newest"].as_array().unwrap().len(), MOST_TAGS);
        assert_eq!(answer["exists"], true);
        assert!(answer["note"].as_str().unwrap().contains("ghcr.io"));
        let answer = look_up(
            &registries,
            &asked(json!({ "image": "quay.io/prometheus/node-exporter", "prefix": "v1." })),
        )
        .unwrap();
        assert_eq!(answer["newest"], json!(["v1.9.1", "v1.8.2"]));
        assert!(registries.asked().iter().all(|(url, _)| allowed(url)));
        let gone = look_up(
            &registries,
            &asked(json!({ "image": "ghcr.io/nobody/nothing" })),
        )
        .unwrap_err();
        assert!(matches!(gone, Failed::NotFound(_)), "{gone:?}");
    }

    #[test]
    fn a_release_is_three_numbers_and_nothing_after() {
        for yes in ["1.2.3", "v1.2.3", "12.4.14", "0.0.0"] {
            assert!(is_release(yes), "{yes}");
        }
        for no in [
            "1.2",
            "1.2.3-alpine",
            "v1.2.3.4",
            "latest",
            "v1",
            "20231010",
            "1.2.3.",
        ] {
            assert!(!is_release(no), "{no}");
        }
    }

    /// The real registries, asked: `cargo test -p endora-capabilities -- --ignored live`.
    #[test]
    #[ignore = "asks the real registries over the network"]
    fn live_registries_answer() {
        for image in [
            "ghcr.io/gethomepage/homepage",
            "grafana/grafana",
            "prom/prometheus",
            "telegraf",
            "matusnovak/prometheus-smartctl",
            "miguelndecarvalho/speedtest-exporter",
            "lscr.io/linuxserver/sonarr",
            "quay.io/prometheus/node-exporter",
        ] {
            let answer = look_up(
                &Web,
                &asked(json!({ "image": image, "semver_only": true, "tag": "latest" })),
            );
            println!("{image}: {answer:?}");
            assert!(answer.is_ok(), "{image}");
        }
    }
}
