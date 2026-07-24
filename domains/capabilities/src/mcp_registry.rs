//! A best-effort lookup against the **community MCP registry** (ADR 0021).
//!
//! Discovery only — it never installs anything. Results are suggestions the person
//! reviews and edits before registering, and anything registered is still
//! deny-by-default (its tools are blocked until allowed, ADR 0024).
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
}

/// Searches `base_url` for `q`. `None` means the lookup didn't work (unreachable,
/// bad JSON, or an unrecognised shape) — callers fall back to the curated catalog.
#[must_use]
pub fn search(base_url: &str, q: &str) -> Option<Vec<RegistryEntry>> {
    let url = if q.trim().is_empty() {
        base_url.to_owned()
    } else {
        format!(
            "{base_url}{}search={}",
            if base_url.contains('?') { '&' } else { '?' },
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
    Some(
        items
            .iter()
            .filter_map(|it| {
                let name = it
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())?;
                let description = it
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let docs = it
                    .get("repository")
                    .and_then(|r| r.get("url"))
                    .and_then(Value::as_str)
                    .or_else(|| it.get("homepage").and_then(Value::as_str))
                    .or_else(|| it.get("url").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_owned();
                Some(RegistryEntry {
                    name: name.to_owned(),
                    description,
                    docs,
                })
            })
            .take(40)
            .collect(),
    )
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
    fn parses_the_common_response_shapes() {
        let wrapped = json!({ "servers": [
            { "name": "acme/files", "description": "files",
              "repository": { "url": "https://example.com/repo" } },
            { "name": "  ", "description": "blank name is skipped" },
        ]});
        let got = parse(&wrapped).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "acme/files");
        assert_eq!(got[0].docs, "https://example.com/repo");

        // A bare array works too, and a missing description is fine.
        let bare = json!([{ "name": "x", "homepage": "https://h" }]);
        let got = parse(&bare).unwrap();
        assert_eq!(got[0].description, "");
        assert_eq!(got[0].docs, "https://h");
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
}
