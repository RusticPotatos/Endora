//! A best-effort lookup against the **community MCP registry** (ADR 0054).
//!
//! Discovery only — it never installs anything. Results are suggestions the person
//! reviews and edits before registering, and anything registered is still
//! deny-by-default (its tools are blocked until allowed, ADR 0051).
//!
//! Deliberately lenient about the response shape: it accepts a bare array or an
//! object wrapping one, and reads whatever name/description/link it can find. A
//! schema change upstream degrades to "no registry results" rather than breaking
//! search — the curated catalog still answers.

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

/// The default community registry endpoint.
pub const DEFAULT_REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io/v0/servers";

/// One server the registry knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The server's name as the registry lists it.
    pub name: String,
    /// Its one-line description (may be empty).
    pub description: String,
    /// A repository/homepage link, if the registry gave one.
    pub docs: String,
    /// `"http"` when the registry lists a remote endpoint, else `"stdio"`.
    pub transport: String,
    /// The remote endpoint, when `transport` is `"http"`.
    pub url: String,
    /// Suggested launch command for a packaged server (e.g. `npx`), else empty.
    pub command: String,
    /// Suggested launch arguments (runtime args followed by the package id).
    pub args: Vec<String>,
    /// Environment variables the package says it needs — offered as blank
    /// `KEY=` lines for the person to fill; values never come from the registry.
    pub env_keys: Vec<String>,
    /// When the registry last saw an update, as the ISO date it publishes (may be
    /// empty). The registry exposes no download count or popularity of any kind, so
    /// recency is the only ordering signal available — and a date at least says whether
    /// a server has been touched this year.
    pub updated: String,
}

/// How many distinct servers to gather before stopping early.
const ENOUGH: usize = 25;

/// Query spellings to try, in order. The registry matches a **substring of the
/// server name**, and names are packed like `io.github.foo/homeassistant-mcp` — so a
/// natural two-word query ("home assistant") matches nothing while "homeassistant"
/// matches plenty. Try the words joined and hyphenated too.
fn query_variants(q: &str) -> Vec<String> {
    let q = q.trim();
    let mut out = vec![q.to_owned()];
    if q.contains(char::is_whitespace) {
        for v in [
            q.split_whitespace().collect::<Vec<_>>().join(""),
            q.split_whitespace().collect::<Vec<_>>().join("-"),
        ] {
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// Searches `base_url` for `q`, trying a few spellings of the query and merging the
/// results (deduped by name). `None` means no lookup worked (unreachable, bad JSON,
/// or an unrecognised shape) — callers fall back to the curated catalog.
#[must_use]
pub fn search(base_url: &str, q: &str) -> Option<Vec<RegistryEntry>> {
    let mut out: Vec<RegistryEntry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut any_ok = false;
    for variant in query_variants(q) {
        // Stop as soon as we have plenty — the extra spellings only exist to rescue a
        // query that found little, so a fruitful first try costs no extra round trip.
        if out.len() >= ENOUGH {
            break;
        }
        let Some(batch) = search_once(base_url, &variant) else {
            continue;
        };
        any_ok = true;
        for e in batch {
            if seen.insert(e.name.clone()) {
                out.push(e);
            }
        }
    }
    // Newest first. The registry publishes no popularity signal at all — no downloads,
    // no stars — so recency is the only ordering available, and a server nobody has
    // touched in two years is the one worth scrolling past. Entries with no date sort
    // last rather than first, so a missing timestamp never masquerades as fresh.
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    any_ok.then_some(out)
}

/// One lookup for one exact query spelling.
fn search_once(base_url: &str, q: &str) -> Option<Vec<RegistryEntry>> {
    let sep = if base_url.contains('?') { '&' } else { '?' };
    // Ask for a full page; the registry's default is small.
    let url = if q.trim().is_empty() {
        format!("{base_url}{sep}limit=100")
    } else {
        format!(
            "{base_url}{sep}limit=100&search={}",
            percent_encode(q.trim())
        )
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .into();
    let mut resp = agent.get(&url).call().ok()?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    let body: Value = serde_json::from_slice(&buf).ok()?;
    parse(&body)
}

/// Pulls entries out of a registry response body.
fn parse(body: &Value) -> Option<Vec<RegistryEntry>> {
    let items = body
        .as_array()
        .or_else(|| body.get("servers").and_then(Value::as_array))
        .or_else(|| body.get("data").and_then(Value::as_array))
        .or_else(|| body.get("results").and_then(Value::as_array))?;
    // The registry lists a row per published version, so the same server can appear
    // several times. Keep the first of each name — the list is for choosing a server,
    // not a version.
    let mut seen = std::collections::HashSet::new();
    Some(
        items
            .iter()
            .filter_map(entry)
            .filter(|e| seen.insert(e.name.clone()))
            .take(40)
            .collect(),
    )
}

/// Reads one registry item. The official registry nests the payload under `server`;
/// older/other shapes put it at the top level, so we accept either. Field names are
/// read in both camelCase and snake_case.
/// When the registry last recorded a change, from the official `_meta` block.
///
/// The registry publishes no download count, star count or popularity of any kind —
/// only these timestamps — so recency is the only ordering signal there is. Returned as
/// the bare `YYYY-MM-DD`, which is all a person scanning a list needs.
fn updated_at(item: &Value) -> String {
    item.get("_meta")
        .and_then(|m| m.get("io.modelcontextprotocol.registry/official"))
        .and_then(|o| {
            o.get("updatedAt")
                .or_else(|| o.get("publishedAt"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .chars()
        .take(10)
        .collect()
}

fn entry(item: &Value) -> Option<RegistryEntry> {
    let s = item.get("server").unwrap_or(item);
    let name = s
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let description = s
        .get("description")
        .or_else(|| s.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let docs = s
        .get("repository")
        .and_then(|r| r.get("url"))
        .and_then(Value::as_str)
        .or_else(|| s.get("websiteUrl").and_then(Value::as_str))
        .or_else(|| s.get("homepage").and_then(Value::as_str))
        .unwrap_or("")
        .to_owned();

    // Prefer a hosted endpoint: nothing to install, just connect.
    let remote_url = s
        .get("remotes")
        .and_then(Value::as_array)
        .and_then(|rs| rs.iter().find_map(|r| r.get("url").and_then(Value::as_str)))
        .map(str::to_owned);
    if let Some(url) = remote_url {
        return Some(RegistryEntry {
            name: name.to_owned(),
            description,
            docs,
            transport: "http".to_owned(),
            url,
            command: String::new(),
            args: Vec::new(),
            env_keys: Vec::new(),
            updated: updated_at(item),
        });
    }

    // Otherwise suggest how to launch the first published package.
    let pkg = s
        .get("packages")
        .and_then(Value::as_array)
        .and_then(|p| p.first());
    let (command, args, env_keys) = pkg.map_or_else(
        || (String::new(), Vec::new(), Vec::new()),
        |p| {
            let field = |a: &str, b: &str| {
                p.get(a)
                    .or_else(|| p.get(b))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let registry_type = field("registryType", "registry_type");
            let hint = field("runtimeHint", "runtime_hint");
            let command = if hint.is_empty() {
                match registry_type.as_str() {
                    "npm" => "npx".to_owned(),
                    "pypi" => "uvx".to_owned(),
                    _ => String::new(),
                }
            } else {
                hint
            };
            // Runtime arguments (e.g. `-y`) come before the package identifier.
            let mut args: Vec<String> = p
                .get("runtimeArguments")
                .or_else(|| p.get("runtime_arguments"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.get("value").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if let Some(id) = p.get("identifier").and_then(Value::as_str) {
                args.push(id.to_owned());
            }
            let env_keys = p
                .get("environmentVariables")
                .or_else(|| p.get("environment_variables"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.get("name").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            (command, args, env_keys)
        },
    );
    Some(RegistryEntry {
        name: name.to_owned(),
        description,
        docs,
        transport: "stdio".to_owned(),
        url: String::new(),
        command,
        args,
        env_keys,
        updated: updated_at(item),
    })
}

/// Minimal percent-encoding for a query value (no new dependency).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{parse, percent_encode};
    use serde_json::json;

    #[test]
    fn parses_the_official_shape_where_the_payload_is_nested_under_server() {
        // The real registry nests each entry under `server` — reading `name` from the
        // top level (as an earlier version did) silently matched nothing.
        let body = json!({ "servers": [
            { "server": { "name": "acme/files", "description": "files",
                          "repository": { "url": "https://example.com/repo" } },
              "_meta": { "ignored": true } },
            { "server": { "name": "  " } },
        ]});
        let got = parse(&body).unwrap();
        assert_eq!(got.len(), 1, "blank names are skipped");
        assert_eq!(got[0].name, "acme/files");
        assert_eq!(got[0].docs, "https://example.com/repo");

        // A flat array (older/other registries) still works.
        let bare = json!([{ "name": "x", "homepage": "https://h" }]);
        let got = parse(&bare).unwrap();
        assert_eq!(got[0].description, "");
        assert_eq!(got[0].docs, "https://h");
    }

    #[test]
    fn a_hosted_remote_becomes_an_http_entry() {
        let body = json!({ "servers": [{ "server": {
            "name": "ac/mcp",
            "description": "hosted",
            "remotes": [{ "type": "streamable-http", "url": "https://api.example/mcp" }],
        }}]});
        let got = parse(&body).unwrap();
        assert_eq!(got[0].transport, "http");
        assert_eq!(got[0].url, "https://api.example/mcp");
        assert!(got[0].command.is_empty());
    }

    #[test]
    fn a_package_becomes_a_runnable_stdio_suggestion() {
        let body = json!({ "servers": [{ "server": {
            "name": "com.x/fs",
            "description": "files",
            "remotes": null,
            "packages": [{
                "registryType": "npm",
                "identifier": "remote-filesystem-mcp-server",
                "runtimeHint": "npx",
                "runtimeArguments": [{ "value": "-y", "type": "positional" }],
                "environmentVariables": [{ "name": "GCS_BUCKET", "isRequired": true }],
            }],
        }}]});
        let got = parse(&body).unwrap();
        assert_eq!(got[0].transport, "stdio");
        assert_eq!(got[0].command, "npx");
        assert_eq!(got[0].args, vec!["-y", "remote-filesystem-mcp-server"]);
        assert_eq!(got[0].env_keys, vec!["GCS_BUCKET"]);
    }

    #[test]
    fn repeated_versions_of_one_server_collapse_to_a_single_entry() {
        let body = json!({ "servers": [
            { "server": { "name": "com.x/fs", "version": "0.1.2" } },
            { "server": { "name": "com.x/fs", "version": "0.1.3" } },
            { "server": { "name": "com.y/other" } },
        ]});
        let got = parse(&body).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "com.x/fs");
        assert_eq!(got[1].name, "com.y/other");
    }

    #[test]
    fn a_pypi_package_without_a_hint_uses_uvx() {
        let body = json!({ "servers": [{ "server": {
            "name": "p/y", "packages": [{ "registry_type": "pypi", "identifier": "mcp-server-x" }],
        }}]});
        let got = parse(&body).unwrap();
        assert_eq!(got[0].command, "uvx");
        assert_eq!(got[0].args, vec!["mcp-server-x"]);
    }

    #[test]
    fn an_unrecognised_shape_is_none_not_a_panic() {
        assert!(parse(&json!({ "unexpected": true })).is_none());
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(percent_encode("home assistant"), "home+assistant");
        assert_eq!(percent_encode("a/b"), "a%2Fb");
    }

    #[test]
    fn multi_word_queries_also_try_joined_and_hyphenated_spellings() {
        // The registry matches a substring of the packed server name, so "home
        // assistant" finds nothing while "homeassistant" finds plenty.
        assert_eq!(
            super::query_variants("home assistant"),
            vec!["home assistant", "homeassistant", "home-assistant"]
        );
        // A single word needs no extra round trips.
        assert_eq!(super::query_variants("github"), vec!["github"]);
        // Whitespace is normalised, not blindly replaced.
        assert_eq!(
            super::query_variants("  home   assistant "),
            vec!["home   assistant", "homeassistant", "home-assistant"]
        );
    }
}
