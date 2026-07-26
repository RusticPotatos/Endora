//! Butler **capabilities** (skills) — the modules the butler can reach for
//! (ADR 0019 §capabilities). Each is a self-contained unit that declares what it
//! does, its **autonomy level** (may it act, or must it ask?), whether it
//! **reaches outside the machine**, and whether it is **configured** (ready, or
//! waiting on a key / model / data source). Consequential or unconfigured skills
//! are surfaced but gated by the policy layer — the butler proposes, the person
//! authorizes; the model is never the enforcement boundary.
//!
//! MCP note: these are the internal `Capability` interface; an MCP server is one
//! way to back a capability (ADR 0019 §3). The registry here is the substrate a
//! future MCP host adapter plugs into.

use std::sync::Arc;
use std::time::Duration;

use crate::application::CapabilityRunner;
use endora_kernel::{Decision, Reversibility};
use serde_json::{Value, json};

/// One setting a capability needs to work (a key, a model name, a URL). Declared
/// in metadata so the console can render a form and the policy layer can tell
/// whether the skill is ready (ADR 0021).
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// Stable key the value is stored under, e.g. `"model"` or `"api_key"`.
    pub key: &'static str,
    /// Human label for the form field.
    pub label: &'static str,
    /// Whether the value is a secret (never echoed back to the client).
    pub secret: bool,
}

/// A capability's stored settings, keyed by [`SettingSpec::key`]. Passed to
/// `invoke` so a skill reads its configuration (model, key, …) at run time.
pub type CapabilitySettings = std::collections::BTreeMap<String, String>;

/// Static metadata describing a capability to the person and the policy layer.
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    /// Stable slug, e.g. `"weather"`.
    pub id: &'static str,
    /// Human name, e.g. `"Weather"`.
    pub name: &'static str,
    /// One-line description of what it does.
    pub description: &'static str,
    /// Grouping for the UI: `information`, `safety`, `travel`, `media`, `presence`.
    pub category: &'static str,
    /// Whether invoking it sends data outside this machine.
    pub reaches_external: bool,
    /// How undoable the skill's effect is — the **primary axis** of the autonomy
    /// envelope (ADR 0024). Declared in metadata, never inferred by a model: it
    /// decides whether policy may run the skill on its own, confirm first, or
    /// block it outright. The classifier NEVER runs an irreversible skill
    /// (deny-by-default).
    pub reversibility: Reversibility,
    /// Whether the code is ready in principle (ignoring settings). Effective
    /// readiness also requires every [`Self::settings`] to have a value.
    pub configured: bool,
    /// If not configured, a short note on what it needs.
    pub needs: &'static str,
    /// The settings this capability needs to run (empty for keyless skills). A
    /// skill is usable only once all of these have values.
    pub settings: &'static [SettingSpec],
}

/// Why a capability call failed.
#[derive(Debug, Clone)]
pub enum CapabilityError {
    /// The capability is not set up (missing key/model/source) or unreachable.
    Unavailable(String),
    /// The input was missing or malformed.
    BadInput(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
            Self::BadInput(m) => write!(f, "bad input: {m}"),
        }
    }
}

/// A butler skill: metadata plus a synchronous `invoke` (run on a blocking
/// worker in the node, like the model calls).
pub trait Capability: Send + Sync {
    /// Static description of this capability.
    fn info(&self) -> CapabilityInfo;

    /// Runs the capability with JSON `input` and its configured `settings`,
    /// returning a JSON result. Keyless skills ignore `settings`.
    ///
    /// # Errors
    /// [`CapabilityError`] if the input is bad or the capability is unavailable.
    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError>;

    /// Renders a result into short, human-readable text for the butler to answer
    /// from. Small local models relay a clean sentence far better than raw JSON,
    /// so each skill that the butler speaks from overrides this. The default is
    /// the JSON itself (fine for programmatic consumers / the Skills UI).
    fn summarize(&self, output: &Value) -> String {
        output.to_string()
    }
}

/// Builds the default set of capabilities the node offers. Read-only information
/// skills are ready; the rest are declared but await configuration, so they show
/// up as modules to enable rather than silently missing.
#[must_use]
pub fn default_capabilities() -> Vec<Arc<dyn Capability>> {
    vec![
        Arc::new(WeatherCapability),
        Arc::new(WebFetchCapability),
        Arc::new(KnowledgeCapability),
        Arc::new(WebAnswersCapability),
        Arc::new(LocalNewsCapability),
        Arc::new(ImageReviewCapability::from_env()),
        Arc::new(LocalEventsCapability),
        Arc::new(FlightSearchCapability),
        Arc::new(LocationLogCapability),
        Arc::new(SafetyAlertsCapability),
        Arc::new(IncidentScannerCapability),
        Arc::new(HomeAssistantCapability),
    ]
}

// ---- helpers ---------------------------------------------------------------

/// A **direct** HTTP agent — no egress proxy. Used for trusted internal calls (the
/// local vision model), which must never be routed through a VPN/proxy.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// The configured egress proxy, if any. `ENDORA_EGRESS_PROXY` is an `http://`,
/// `https://`, or `socks5://` URL (trusted deployment config). See
/// [`external_agent`].
fn egress_proxy() -> Option<ureq::Proxy> {
    let url = std::env::var("ENDORA_EGRESS_PROXY").ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    ureq::Proxy::new(url).ok()
}

/// An agent for **external** skill egress. If an egress proxy is configured,
/// outbound skill requests route through it — so you can send them via a VPN's
/// proxy (e.g. gluetun's HTTP proxy) **without** binding Endora's network to the VPN
/// container (ADR 0023). Loosely coupled: if the proxy is down, only external skills
/// fail; the app keeps running.
fn external_agent() -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15)));
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    builder.build().into()
}

/// GETs a URL and returns the body as text (size-capped), for the info skills.
fn http_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = external_agent()
        .get(url)
        .call()
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn str_field<'a>(input: &'a Value, key: &str) -> Result<&'a str, CapabilityError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CapabilityError::BadInput(format!("missing '{key}'")))
}

/// GET with an explicit `User-Agent` (some APIs, e.g. the US NWS, require one).
fn http_get_text_ua(url: &str, ua: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = external_agent()
        .get(url)
        .header("User-Agent", ua)
        .call()
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// ---- Egress guard (SSRF protection for model/person-provided URLs) ----------

/// Rejects a URL whose host is, or resolves to, a non-public address — closing the
/// SSRF hole where a model-provided URL could reach the internal network or a cloud
/// metadata endpoint (ADR 0023). Only for **arbitrary** URLs; the trusted internal
/// model calls and constant-host API skills do not use this.
fn guard_egress(url: &str) -> Result<(), CapabilityError> {
    let (host, port) = host_and_port(url)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't parse URL host: {url}")))?;
    let deny = |ip: &std::net::IpAddr| {
        // Anything that isn't a normal public address is refused.
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || v4.is_multicast()
                    || v4.is_documentation()
                    // Carrier-grade NAT 100.64.0.0/10.
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 0x40)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    // Unique-local fc00::/7 and link-local fe80::/10.
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        }
    };
    let blocked = |ip: std::net::IpAddr| {
        if deny(&ip) {
            Err(CapabilityError::BadInput(format!(
                "refusing to fetch a private/internal address ({ip})"
            )))
        } else {
            Ok(())
        }
    };
    // A literal IP is checked directly; a hostname is resolved first (so a name that
    // points at an internal IP is caught too).
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return blocked(ip);
    }
    use std::net::ToSocketAddrs;
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| CapabilityError::Unavailable(format!("couldn't resolve {host}: {e}")))?;
    let mut any = false;
    for a in addrs {
        any = true;
        blocked(a.ip())?;
    }
    if any {
        Ok(())
    } else {
        Err(CapabilityError::Unavailable(format!(
            "couldn't resolve {host}"
        )))
    }
}

/// Extracts `(host, port)` from an http(s) URL, without a URL dependency. Strips
/// userinfo, handles IPv6 literals `[..]`, and defaults the port by scheme.
fn host_and_port(url: &str) -> Option<(String, u16)> {
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end];
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let after = &url[scheme_end + 3..];
    let authority = after.split(['/', '?', '#']).next()?;
    // Strip any userinfo ("user:pass@host").
    let authority = authority.rsplit('@').next()?;
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal: [addr] or [addr]:port.
        let end = rest.find(']')?;
        let host = rest[..end].to_owned();
        let port = rest[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse().ok())
            .unwrap_or(default_port);
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            Some((host.to_owned(), port.parse().unwrap_or(default_port)))
        }
        _ => Some((authority.to_owned(), default_port)),
    }
}

/// The data-loss tripwire (ADR 0023): scans text about to leave the machine (an
/// external skill's input) for **high-confidence secrets**, so the butler can't be
/// steered into leaking a key or private key in a query. Deliberately precise —
/// only well-known credential shapes — to avoid blocking legitimate requests.
/// Returns a short label of what was found, or `None` if the text looks clean.
pub fn scan_outbound_secret(text: &str) -> Option<&'static str> {
    if text.contains("PRIVATE KEY-----") {
        return Some("a private key");
    }
    // Split on whitespace, JSON/markup punctuation, AND URL delimiters, so a secret
    // embedded in a query string (…?token=sk-…) is isolated as its own token.
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ','
                    | ';'
                    | '\\'
                    | '='
                    | '?'
                    | '&'
                    | '/'
                    | ':'
                    | '#'
                    | '@'
                    | '|'
            )
    })
    .find_map(classify_secret_token)
}

/// Query minimization (ADR 0023): redacts personal identifiers from an external
/// skill's input before it leaves — so a search doesn't carry a real email address
/// out. Recurses through the JSON, redacting string values in place. Deliberately
/// narrow (email addresses) and word-boundaried, so URLs and ordinary text survive.
pub fn redact_pii_in_value(v: &mut Value) {
    match v {
        Value::String(s) => *s = redact_emails_in_text(s),
        Value::Array(a) => a.iter_mut().for_each(redact_pii_in_value),
        Value::Object(o) => o.values_mut().for_each(redact_pii_in_value),
        _ => {}
    }
}

/// Replaces whole-word email addresses in free text with `[redacted-email]`. Splits
/// on spaces so a bare URL (one word, not an email) is never altered — only a
/// standalone address in a query is caught.
fn redact_emails_in_text(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let core = word.trim_matches(|c: char| {
                !(c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+'))
            });
            if !core.is_empty() && looks_like_email(core) {
                word.replace(core, "[redacted-email]")
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a token is a bare email address (and not a URL).
fn looks_like_email(s: &str) -> bool {
    if s.contains('/') || s.contains(':') {
        return false;
    }
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && domain
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Classifies a single token as a known credential shape, or `None`.
fn classify_secret_token(t: &str) -> Option<&'static str> {
    let n = t.len();
    // Tail after a prefix is credential-like (alnum plus `_`/`-`).
    let tail_ok = |prefix: &str, min: usize| {
        t.len() >= min
            && t.starts_with(prefix)
            && t[prefix.len()..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    // AWS access key id: AKIA + 16 uppercase/digits.
    if n == 20
        && t.starts_with("AKIA")
        && t[4..]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some("an AWS access key");
    }
    if tail_ok("sk-ant-", 24) || tail_ok("sk-", 20) {
        return Some("an API key");
    }
    if tail_ok("github_pat_", 22)
        || tail_ok("ghp_", 20)
        || tail_ok("gho_", 20)
        || tail_ok("ghs_", 20)
    {
        return Some("a GitHub token");
    }
    if (t.starts_with("xoxb-") || t.starts_with("xoxp-") || t.starts_with("xoxa-")) && n >= 24 {
        return Some("a Slack token");
    }
    if (t.starts_with("sk_live_") || t.starts_with("rk_live_")) && n >= 20 {
        return Some("a Stripe key");
    }
    if n == 39 && t.starts_with("AIza") {
        return Some("a Google API key");
    }
    // JWT: three base64url segments separated by dots.
    if t.starts_with("eyJ") {
        let parts: Vec<&str> = t.split('.').collect();
        let is_b64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=';
        if parts.len() == 3
            && parts
                .iter()
                .all(|p| p.len() >= 8 && p.chars().all(is_b64url))
        {
            return Some("a token (JWT)");
        }
    }
    None
}

/// Like [`http_get_text`], but guards the URL against SSRF and follows redirects
/// manually, re-guarding each hop (ADR 0023). For model/person-provided URLs.
fn guarded_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    let bytes = guarded_get_bytes(url, max_bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Guarded byte fetch with manual, re-guarded redirect following (bounded).
fn guarded_get_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, CapabilityError> {
    use std::io::Read;
    // No auto-redirects (we re-guard each hop), and honour the egress proxy.
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0);
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    let no_redirect: ureq::Agent = builder.build().into();
    let mut current = url.to_owned();
    for _ in 0..6 {
        guard_egress(&current)?;
        let mut resp = no_redirect
            .get(&current)
            .header("User-Agent", "Mozilla/5.0 (Endora personal butler)")
            .call()
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    CapabilityError::Unavailable("redirect without location".to_owned())
                })?
                .to_owned();
            current = resolve_redirect(&current, &location);
            continue;
        }
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(max_bytes as u64)
            .read_to_end(&mut buf)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        return Ok(buf);
    }
    Err(CapabilityError::Unavailable(
        "too many redirects".to_owned(),
    ))
}

/// Resolves a redirect `Location` (absolute or root-relative) against the URL it
/// came from. Root-relative and same-scheme cases are enough for our fetches.
fn resolve_redirect(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_owned();
    }
    // Root-relative: keep scheme://host[:port], swap the path.
    if let Some(scheme_end) = base.find("://") {
        let after = &base[scheme_end + 3..];
        let authority_len = after.find('/').unwrap_or(after.len());
        let origin = &base[..scheme_end + 3 + authority_len];
        if location.starts_with('/') {
            return format!("{origin}{location}");
        }
        return format!("{origin}/{location}");
    }
    location.to_owned()
}

/// POSTs a JSON body and returns the response as text (size-capped).
fn http_post_json(url: &str, body: &Value, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = agent()
        .post(url)
        .send_json(body)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Standard base64 encoding (no dependency) — for embedding image bytes in a JSON
/// request to a local vision model.
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Resolves `{lat, lon}` or `{location}` from an input to a `(lat, lon, place)`,
/// geocoding a place name via Open-Meteo (no key). Shared by the location skills.
fn resolve_point(input: &Value) -> Result<(f64, f64, String), CapabilityError> {
    if let (Some(lat), Some(lon)) = (
        input.get("lat").and_then(Value::as_f64),
        input.get("lon").and_then(Value::as_f64),
    ) {
        let place = input
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        return Ok((lat, lon, place));
    }
    let q = str_field(input, "location")?;
    // A bare US ZIP ("10001") — common when the person types their postcode — is
    // not resolvable by the place-name geocoder, so use a keyless ZIP lookup.
    if let Some(point) = resolve_us_zip(q)? {
        return Ok(point);
    }
    // The Open-Meteo geocoder wants a bare city name — it returns nothing for
    // "New York NC" or "New York, NC". So try the full query, then simpler forms
    // (before a comma, and without a trailing US state abbreviation).
    let first = geocode_candidates(q)
        .into_iter()
        .find_map(|cand| {
            let geo = http_get_text(
                &format!(
                    "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                    urlencode(&cand)
                ),
                64 * 1024,
            )
            .ok()?;
            let geo: Value = serde_json::from_str(&geo).ok()?;
            geo["results"].get(0).cloned()
        })
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find a place called '{q}'")))?;
    let lat = first["latitude"].as_f64().unwrap_or_default();
    let lon = first["longitude"].as_f64().unwrap_or_default();
    let name = first["name"].as_str().unwrap_or(q);
    let country = first["country"].as_str().unwrap_or("");
    Ok((
        lat,
        lon,
        format!("{name}, {country}")
            .trim_end_matches(", ")
            .to_owned(),
    ))
}

/// Progressively simpler place-name queries for the geocoder: the full string, the
/// part before a comma, and the same without a trailing 2-letter US state (so
/// "New York NC" / "New York, NC" both fall back to "New York").
fn geocode_candidates(q: &str) -> Vec<String> {
    let mut out = vec![q.trim().to_owned()];
    let before_comma = q.split(',').next().unwrap_or(q).trim();
    let toks: Vec<&str> = before_comma.split_whitespace().collect();
    let simplified = if toks.len() >= 2
        && toks
            .last()
            .is_some_and(|t| t.len() == 2 && t.chars().all(|c| c.is_ascii_uppercase()))
    {
        toks[..toks.len() - 1].join(" ")
    } else {
        before_comma.to_owned()
    };
    if !simplified.is_empty() && !out.contains(&simplified) {
        out.push(simplified);
    }
    out
}

/// If `q` is a 5-digit US ZIP, resolve it to `(lat, lon, "City, ST")` via the
/// keyless zippopotam.us service. Returns `Ok(None)` when `q` isn't a ZIP so the
/// caller falls through to the place-name geocoder.
fn resolve_us_zip(q: &str) -> Result<Option<(f64, f64, String)>, CapabilityError> {
    let zip = q.trim();
    if zip.len() != 5 || !zip.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let body = http_get_text(&format!("https://api.zippopotam.us/us/{zip}"), 16 * 1024)?;
    let data: Value =
        serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let place = data["places"]
        .get(0)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find the ZIP code '{zip}'")))?;
    // zippopotam returns latitude/longitude as strings.
    let lat = place["latitude"].as_str().and_then(|s| s.parse().ok());
    let lon = place["longitude"].as_str().and_then(|s| s.parse().ok());
    let (Some(lat), Some(lon)) = (lat, lon) else {
        return Ok(None);
    };
    let city = place["place name"].as_str().unwrap_or("");
    let state = place["state abbreviation"].as_str().unwrap_or("");
    Ok(Some((
        lat,
        lon,
        format!("{city}, {state}")
            .trim_matches([',', ' '])
            .to_owned(),
    )))
}

// ---- Weather (real; Open-Meteo, no API key) --------------------------------

struct WeatherCapability;

impl Capability for WeatherCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "weather",
            name: "Weather",
            description: "Current conditions and today's forecast for a place, with a heads-up on severe weather.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Accept {location:"..."} (a place name or US ZIP) or {lat, lon}.
        let (lat, lon, place) = resolve_point(input)?;

        let body = http_get_text(
            &format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m\
                 &daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=1"
            ),
            64 * 1024,
        )?;
        let w: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let code = w["current"]["weather_code"].as_i64().unwrap_or(-1);
        let condition = weather_condition(code);
        let severe = matches!(code, 65 | 75 | 82 | 86 | 95 | 96 | 99);
        Ok(json!({
            "place": place,
            "temperature_c": w["current"]["temperature_2m"],
            "feels_like_c": w["current"]["apparent_temperature"],
            "wind_kmh": w["current"]["wind_speed_10m"],
            "condition": condition,
            "high_c": w["daily"]["temperature_2m_max"][0],
            "low_c": w["daily"]["temperature_2m_min"][0],
            // The local time this reading is FROM — a brief is posted once and never
            // updates, so "current" can go stale by the time it's read. Surfacing the
            // observation time makes that obvious ("as of 7 AM") instead of looking
            // like a wrong right-now temperature.
            "observed_at": w["current"]["time"],
            "warning": if severe { format!("Heads-up: {condition} expected — take care.") } else { String::new() },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let cond = o["condition"].as_str().unwrap_or("");
        // Emit BOTH units so the model relays a grounded number instead of doing
        // its own (error-prone) C↔F conversion — whichever unit the person prefers,
        // the real value is right here.
        let cf = |c: f64| format!("{c:.0}°C / {:.0}°F", c * 9.0 / 5.0 + 32.0);
        let mut s = format!("Weather for {place}: {cond}");
        if let Some(t) = o["temperature_c"].as_f64() {
            s.push_str(&format!(", {}", cf(t)));
        }
        if let Some(f) = o["feels_like_c"].as_f64() {
            s.push_str(&format!(" (feels like {})", cf(f)));
        }
        // Time-stamp the reading so a stale brief reads as "as of this morning",
        // not a wrong current temperature.
        if let Some(at) = o["observed_at"].as_str().and_then(observed_time) {
            s.push_str(&format!(" as of {at}"));
        }
        if let (Some(hi), Some(lo)) = (o["high_c"].as_f64(), o["low_c"].as_f64()) {
            s.push_str(&format!("; today's high {}, low {}", cf(hi), cf(lo)));
        }
        if let Some(w) = o["warning"].as_str().filter(|w| !w.is_empty()) {
            s.push_str(&format!(". {w}"));
        }
        s
    }
}

/// Formats an Open-Meteo local ISO timestamp (`2026-07-23T07:00`) as a friendly
/// 12-hour time (`7:00 AM`), for the "as of …" tag on a weather reading. Returns
/// `None` if the shape isn't recognized.
fn observed_time(iso: &str) -> Option<String> {
    let hm = iso.split('T').nth(1)?;
    let (h, m) = hm.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    if h > 23 || m.len() != 2 || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ampm = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        x => x,
    };
    Some(format!("{h12}:{m} {ampm}"))
}

fn weather_condition(code: i64) -> &'static str {
    match code {
        0 => "clear sky",
        1..=3 => "partly cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 63 | 66 | 67 | 80 | 81 => "rain",
        65 | 82 => "heavy rain",
        71 | 73 | 77 | 85 => "snow",
        75 | 86 => "heavy snow",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

// ---- Web fetch / browse (real) ---------------------------------------------

struct WebFetchCapability;

impl Capability for WebFetchCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_fetch",
            name: "Web browsing",
            description: "Fetch a web page and read its text — for research and briefings.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let url = str_field(input, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CapabilityError::BadInput("url must be http(s)".to_owned()));
        }
        // Guard against SSRF: a model-provided URL must not reach internal hosts.
        let html = guarded_get_text(url, 512 * 1024)?;
        let title = between(&html, "<title>", "</title>").unwrap_or_default();
        let text = strip_html(&html);
        let excerpt: String = text.chars().take(2_000).collect();
        Ok(json!({ "url": url, "title": title.trim(), "text": excerpt }))
    }
}

fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(s[start..end].to_owned())
}

/// Very small HTML-to-text: drops script/style, strips tags, collapses space.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(close) = skip_to {
            if lower[i..].starts_with(close) {
                skip_to = None;
                i += close.len();
            } else {
                i += 1;
            }
            continue;
        }
        if lower[i..].starts_with("<script") {
            skip_to = Some("</script>");
            continue;
        }
        if lower[i..].starts_with("<style") {
            skip_to = Some("</style>");
            continue;
        }
        let c = bytes[i] as char;
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- Local news (real; Google News RSS, no API key) ------------------------

struct LocalNewsCapability;

impl Capability for LocalNewsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "news",
            name: "Local news",
            description: "Recent news headlines for a place or topic — so answers about the news are real, not guessed.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Prefer an explicit {query}; else build one from {location}. A bare ZIP or
        // raw coordinates make a poor news search, so resolve the location to a
        // place name first ("10001" → "New York, NC news"). One of query/location
        // is required — without it we say so rather than invent headlines.
        let query = match input.get("query").and_then(Value::as_str) {
            Some(q) if !q.trim().is_empty() => q.trim().to_owned(),
            _ => {
                let raw = str_field(input, "location").map_err(|_| {
                    CapabilityError::BadInput("missing 'location' or 'query'".to_owned())
                })?;
                let place = match resolve_point(input) {
                    Ok((_, _, p)) if !p.is_empty() => p,
                    _ => raw.to_owned(),
                };
                format!("{place} news")
            }
        };
        let url = format!(
            "https://news.google.com/rss/search?q={}&hl=en-US&gl=US&ceid=US:en",
            urlencode(&query)
        );
        let xml = http_get_text(&url, 256 * 1024)?;
        let items = extract_rss_items(&xml, 6);
        let headlines: Vec<Value> = items
            .iter()
            .map(|(title, link, publisher)| {
                json!({ "title": title, "url": link, "publisher": publisher })
            })
            .collect();
        Ok(json!({
            "query": query,
            "count": headlines.len(),
            "headlines": headlines,
            "note": if headlines.is_empty() {
                "No recent headlines found for that search."
            } else {
                ""
            },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let query = o["query"].as_str().unwrap_or("that");
        // A headline is either a plain string (legacy) or `{title, url}`. Include
        // the source URL when present so the butler can cite it, not guess it.
        let render = |i: usize, h: &Value| -> Option<String> {
            if let Some(t) = h.as_str() {
                return Some(format!("{}. {t}", i + 1));
            }
            let title = h["title"].as_str().filter(|s| !s.is_empty())?;
            let publisher = h["publisher"].as_str().filter(|s| !s.is_empty());
            let url = h["url"].as_str().filter(|s| !s.is_empty());
            // Cite the outlet + the link when we have them, so the butler relays a
            // real source rather than guessing one.
            let src = match (publisher, url) {
                (Some(p), Some(u)) => format!(" — {p} ({u})"),
                (Some(p), None) => format!(" — {p}"),
                (None, Some(u)) => format!(" — {u}"),
                (None, None) => String::new(),
            };
            Some(format!("{}. {title}{src}", i + 1))
        };
        let list: Vec<String> = o["headlines"]
            .as_array()
            .map(|a| {
                a.iter()
                    .enumerate()
                    .filter_map(|(i, h)| render(i, h))
                    .collect()
            })
            .unwrap_or_default();
        if list.is_empty() {
            return format!("No recent news headlines were found for {query}.");
        }
        format!("Recent news headlines for {query}:\n{}", list.join("\n"))
    }
}

/// Extracts up to `max` `<item>` headlines from an RSS feed as `(title, link,
/// publisher)`, decoding the common XML entities. The link is the article's
/// **source** and `publisher` the outlet name (Google News RSS carries a
/// `<source url="…">Publisher</source>`) — both kept so the butler can *cite* a
/// real source, not guess one. Small and tolerant.
fn extract_rss_items(xml: &str, max: usize) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        rest = &rest[start + "<item>".len()..];
        let item = match rest.find("</item>") {
            Some(end) => &rest[..end],
            None => rest,
        };
        let title = between(item, "<title>", "</title>")
            .map(|t| decode_xml_entities(strip_cdata(&t).trim()))
            .unwrap_or_default();
        let link = between(item, "<link>", "</link>")
            .map(|l| decode_xml_entities(strip_cdata(&l).trim()))
            .unwrap_or_default();
        // <source url="…">Publisher</source> — take the text after the opening
        // tag's ">" (skips the url= attribute) up to the closing tag.
        let publisher = between(item, "<source", "</source>")
            .and_then(|s| s.split_once('>').map(|(_, name)| name.to_owned()))
            .map(|n| decode_xml_entities(strip_cdata(n.trim()).trim()))
            .unwrap_or_default();
        if !title.is_empty() {
            out.push((title, link, publisher));
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Unwraps a `<![CDATA[…]]>` wrapper if present, returning the inner text.
fn strip_cdata(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
        .unwrap_or(t)
        .to_owned()
}

/// Decodes the handful of XML entities that show up in RSS titles.
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ---- Knowledge lookup (real; Wikipedia, no API key) ------------------------

struct KnowledgeCapability;

impl Capability for KnowledgeCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "knowledge",
            name: "Knowledge lookup",
            description: "Look up factual, encyclopedic knowledge about a topic, person, or place (Wikipedia).",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let q = str_field(input, "query").or_else(|_| str_field(input, "topic"))?;
        // Find the best-matching article title, then fetch its summary.
        let search = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&srlimit=1&format=json",
                urlencode(q)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let search: Value = serde_json::from_str(&search)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let Some(title) = search["query"]["search"]
            .get(0)
            .and_then(|s| s["title"].as_str())
        else {
            return Ok(json!({ "query": q, "found": false, "extract": "" }));
        };
        let summary = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
                urlencode(title)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let summary: Value = serde_json::from_str(&summary)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({
            "query": q,
            "found": true,
            "title": summary["title"].as_str().unwrap_or(title),
            "extract": summary["extract"].as_str().unwrap_or(""),
            "url": summary["content_urls"]["desktop"]["page"].as_str().unwrap_or(""),
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let extract = o["extract"].as_str().unwrap_or("");
        if extract.is_empty() {
            return format!(
                "I couldn't find an encyclopedia entry for '{}'.",
                o["query"].as_str().unwrap_or("that")
            );
        }
        let title = o["title"].as_str().unwrap_or("");
        format!("{title}: {extract}")
    }
}

const WIKI_UA: &str = "Endora personal butler (github.com/RusticPotatos/Endora)";

// ---- Web answers (real; DuckDuckGo Instant Answer, no API key) --------------

struct WebAnswersCapability;

impl Capability for WebAnswersCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_search",
            name: "Web answers",
            description: "Get a quick answer or definition from the web for a question (DuckDuckGo).",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let q = str_field(input, "query").or_else(|_| str_field(input, "question"))?;
        let body = http_get_text(
            &format!(
                "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
                urlencode(q)
            ),
            256 * 1024,
        )?;
        let data: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        // Prefer a direct answer/abstract; else surface a few related topics.
        let answer = first_non_empty(&[
            data["Answer"].as_str(),
            data["AbstractText"].as_str(),
            data["Definition"].as_str(),
        ]);
        let related: Vec<String> = data["RelatedTopics"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t["Text"].as_str())
                    .filter(|s| !s.is_empty())
                    .take(4)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "query": q,
            "answer": answer,
            "source": data["AbstractURL"].as_str().unwrap_or(""),
            "related": related,
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let query = o["query"].as_str().unwrap_or("that");
        if let Some(answer) = o["answer"].as_str().filter(|s| !s.is_empty()) {
            return answer.to_owned();
        }
        let related: Vec<&str> = o["related"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if related.is_empty() {
            return format!(
                "I didn't find a direct answer for '{query}'. It may need a more specific search."
            );
        }
        format!("Here's what I found for '{query}': {}", related.join("; "))
    }
}

/// The first of several optional strings that is present and non-empty.
fn first_non_empty(candidates: &[Option<&str>]) -> String {
    candidates
        .iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_owned()
}

// ---- Image review (local vision model via Ollama; env-gated) ---------------

struct ImageReviewCapability {
    /// Base URL of the local Ollama (native API), e.g. `http://host:11434`.
    ollama_base: String,
}

impl ImageReviewCapability {
    fn from_env() -> Self {
        // Reuse the butler's model endpoint; the native /api lives at the base, so
        // strip the OpenAI-compat `/v1` suffix.
        let base = std::env::var("ENDORA_MODEL_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
        let base = base
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_owned();
        Self { ollama_base: base }
    }
}

const IMAGE_MODEL_SETTING: &[SettingSpec] = &[SettingSpec {
    key: "model",
    label: "Vision model (a pulled Ollama model, e.g. moondream or llava)",
    secret: false,
}];

impl Capability for ImageReviewCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "image_review",
            name: "Image review",
            description: "Describe or answer questions about an image, using a local vision model.",
            category: "media",
            reaches_external: false,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            // Code is ready; it becomes usable once the `model` setting is filled.
            configured: true,
            needs: "set the vision model (e.g. moondream) in this skill's settings",
            settings: IMAGE_MODEL_SETTING,
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let model = settings
            .get("model")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "no vision model set — configure 'model' (e.g. moondream) in this skill's settings".to_owned(),
                )
            })?;
        let question = input
            .get("question")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Describe this image in detail.");
        // Accept a URL to fetch, or already-encoded base64.
        let image_b64 = if let Some(url) = input.get("image_url").and_then(Value::as_str) {
            // Guard against SSRF on a model/person-provided image URL.
            base64_encode(&guarded_get_bytes(url, 8 * 1024 * 1024)?)
        } else if let Some(b64) = input.get("image_base64").and_then(Value::as_str) {
            b64.to_owned()
        } else {
            return Err(CapabilityError::BadInput(
                "provide 'image_url' or 'image_base64'".to_owned(),
            ));
        };
        let body = http_post_json(
            &format!("{}/api/generate", self.ollama_base),
            &json!({ "model": model, "prompt": question, "images": [image_b64], "stream": false }),
            256 * 1024,
        )?;
        let v: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({ "description": v["response"].as_str().unwrap_or("").trim() }))
    }

    fn summarize(&self, o: &Value) -> String {
        let d = o["description"].as_str().unwrap_or("").trim();
        if d.is_empty() {
            "I couldn't make out anything from that image.".to_owned()
        } else {
            d.to_owned()
        }
    }
}

// ---- Declared-but-unconfigured modules (scaffolds) -------------------------

/// A skill that is declared with its full metadata but awaits a data source or
/// key. It appears in the registry as "needs setup" rather than being missing.
macro_rules! scaffold {
    ($ty:ident, $id:literal, $name:literal, $desc:literal, $cat:literal, $external:literal, $reversibility:expr, $needs:literal) => {
        struct $ty;
        impl Capability for $ty {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: $id,
                    name: $name,
                    description: $desc,
                    category: $cat,
                    reaches_external: $external,
                    reversibility: $reversibility,
                    configured: false,
                    needs: $needs,
                    settings: &[],
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Err(CapabilityError::Unavailable(format!(
                    "{} needs setup: {}",
                    $name, $needs
                )))
            }
        }
    };
}

scaffold!(
    LocalEventsCapability,
    "local_events",
    "Local events",
    "What's on near you — concerts, markets, community happenings.",
    "information",
    true,
    Reversibility::Reversible, // read-only lookup
    "an events data source / API key"
);
scaffold!(
    FlightSearchCapability,
    "flights",
    "Flight search",
    "Find and compare flights for a trip.",
    "travel",
    true,
    Reversibility::Irreversible, // booking spends money and can't be undone
    "a flights API key (booking stays a human decision)"
);
scaffold!(
    LocationLogCapability,
    "location",
    "Location tracking",
    "Keep a private log of where you are while travelling, so the butler has context.",
    "presence",
    false,
    Reversibility::OutwardReversible, // a private log you can delete
    "your opt-in and a location source (kept private to you)"
);
/// The "guard dog": active public-safety alerts near a place. Real for the US via
/// the National Weather Service (no key); elsewhere it reports no coverage.
struct SafetyAlertsCapability;

impl Capability for SafetyAlertsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "safety_alerts",
            name: "Guard dog",
            description: "Active safety alerts near you — severe weather and public warnings (US National Weather Service).",
            category: "safety",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let (lat, lon, place) = resolve_point(input)?;
        let body = http_get_text_ua(
            &format!("https://api.weather.gov/alerts/active?point={lat:.4},{lon:.4}"),
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            256 * 1024,
        )?;
        let data: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let alerts: Vec<Value> = data["features"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        let p = &f["properties"];
                        Some(json!({
                            "event": p["event"].as_str()?,
                            "severity": p["severity"].as_str().unwrap_or("Unknown"),
                            "headline": p["headline"].as_str().unwrap_or(""),
                            "area": p["areaDesc"].as_str().unwrap_or(""),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "place": place,
            "count": alerts.len(),
            "all_clear": alerts.is_empty(),
            "alerts": alerts,
            "note": if alerts.is_empty() { "No active alerts (or outside US coverage)." } else { "" },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let alerts: Vec<String> = o["alerts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        let event = x["event"].as_str()?;
                        let sev = x["severity"].as_str().unwrap_or("");
                        Some(if sev.is_empty() {
                            event.to_owned()
                        } else {
                            format!("{event} ({sev})")
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if alerts.is_empty() {
            return format!("No active safety alerts for {place} right now.");
        }
        format!("Active safety alerts for {place}: {}.", alerts.join("; "))
    }
}

scaffold!(
    IncidentScannerCapability,
    "incident_scanner",
    "Incident scanner",
    "Surface public emergency/incident alerts nearby (fire, rescue, major incidents).",
    "safety",
    true,
    Reversibility::Reversible, // read-only lookup
    "a public incident/emergency feed for your area"
);

// ---- Home Assistant (read-only; learn the home's routines) -----------------

const HA_SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "url",
        label: "Home Assistant URL (e.g. http://homeassistant.local:8123)",
        secret: false,
    },
    SettingSpec {
        key: "token",
        label: "Long-lived access token",
        secret: true,
    },
];

/// Reads Home Assistant state so the butler can learn the home's routines (lights,
/// presence, sensors). Read-only and reversible — it observes, it does not actuate;
/// controlling devices/scripts is a separate, confirm-gated capability (ADR 0024).
struct HomeAssistantCapability;

impl Capability for HomeAssistantCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "home_assistant",
            name: "Home Assistant",
            description: "Read your home's state — lights, presence, sensors — to learn your routines.",
            category: "presence",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0034.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "your Home Assistant URL and a long-lived access token",
            settings: HA_SETTINGS,
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let base = settings
            .get("url")
            .map(|s| s.trim().trim_end_matches('/'))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set the Home Assistant URL in this skill's settings".to_owned(),
                )
            })?;
        let token = settings
            .get("token")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set a Home Assistant access token in this skill's settings".to_owned(),
                )
            })?;
        // Optional {domain} filter (e.g. "light", "person", "sensor", "media_player").
        let domain = input.get("domain").and_then(Value::as_str).unwrap_or("");
        // HA is the person's own local service — direct agent (not proxied/guarded).
        use std::io::Read;
        let mut resp = agent()
            .get(&format!("{base}/api/states"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let states: Value = serde_json::from_str(&String::from_utf8_lossy(&buf))
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let entities: Vec<Value> = states
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|e| {
                        domain.is_empty()
                            || e["entity_id"]
                                .as_str()
                                .is_some_and(|id| id.starts_with(&format!("{domain}.")))
                    })
                    .filter_map(|e| {
                        let id = e["entity_id"].as_str()?;
                        let name = e["attributes"]["friendly_name"].as_str().unwrap_or(id);
                        Some(json!({
                            "entity": id,
                            "name": name,
                            "state": e["state"].as_str().unwrap_or("?"),
                            "changed": e["last_changed"].as_str().unwrap_or(""),
                        }))
                    })
                    .take(60)
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({ "domain": domain, "count": entities.len(), "entities": entities }))
    }

    fn summarize(&self, o: &Value) -> String {
        let entities: Vec<&Value> = o["entities"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        if entities.is_empty() {
            return "No matching Home Assistant entities found.".to_owned();
        }
        let list = entities
            .iter()
            .take(30)
            .map(|e| {
                format!(
                    "{}: {}",
                    e["name"].as_str().unwrap_or("?"),
                    e["state"].as_str().unwrap_or("?")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "Home state ({} entities): {list}",
            o["count"].as_i64().unwrap_or(0)
        )
    }
}

// ---- Application-facing runner ---------------------------------------------

/// Adapts the concrete capability registry to the application's
/// [`CapabilityRunner`] port, so the butler use case can list and run skills
/// without depending on this crate. A capability is "autonomous" (may run on its
/// own this turn) exactly when the classifier's verdict for its [`Reversibility`]
/// band and the person's envelope is [`Act`](Decision::Act). Anything that must
/// confirm — or is blocked — stays gated.
pub struct RegistryRunner {
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// Per-capability enabled overrides (id → enabled). Missing = default enabled.
    enabled: std::collections::HashMap<String, bool>,
    /// Per-capability irreversible-band openers (id → opened, ADR 0024). Missing =
    /// closed: the un-undoable stays blocked until the person opens it.
    opened: std::collections::HashMap<String, bool>,
    /// Per-capability "ask first" overrides (id → confirm). When set, the skill runs
    /// only after the person confirms each use — never on its own — whatever its band.
    confirm: std::collections::HashMap<String, bool>,
    /// The person's autonomy envelope — the boundary the butler acts within.
    envelope: crate::application::AutonomyEnvelope,
    /// Per-capability settings (id → key/value), for skills that need config.
    settings: std::collections::HashMap<String, CapabilitySettings>,
}

impl RegistryRunner {
    /// Wraps a shared capability registry at its defaults (every skill enabled, the
    /// default autonomy envelope, no settings).
    #[must_use]
    pub fn new(capabilities: Arc<Vec<Arc<dyn Capability>>>) -> Self {
        Self {
            capabilities,
            enabled: std::collections::HashMap::new(),
            opened: std::collections::HashMap::new(),
            confirm: std::collections::HashMap::new(),
            envelope: crate::application::AutonomyEnvelope::default(),
            settings: std::collections::HashMap::new(),
        }
    }

    /// Wraps the registry, applying the person's enable/disable overrides (ADR 0021),
    /// their autonomy envelope (ADR 0022), per-capability irreversible-band openers
    /// (ADR 0024), and per-capability settings (ADR 0021). A disabled skill never
    /// runs; the envelope and openers decide which kinds of action may run without
    /// confirmation (or at all); settings make a configurable skill usable.
    #[must_use]
    pub fn with_config(
        capabilities: Arc<Vec<Arc<dyn Capability>>>,
        overrides: Vec<(String, bool)>,
        opened: Vec<(String, bool)>,
        confirm: Vec<(String, bool)>,
        envelope: crate::application::AutonomyEnvelope,
        settings: std::collections::HashMap<String, CapabilitySettings>,
    ) -> Self {
        Self {
            capabilities,
            enabled: overrides.into_iter().collect(),
            opened: opened.into_iter().collect(),
            confirm: confirm.into_iter().collect(),
            envelope,
            settings,
        }
    }

    /// Whether a capability is enabled (its override, or its built-in default).
    fn is_enabled(&self, id: &str) -> bool {
        self.enabled.get(id).copied().unwrap_or(true)
    }

    /// Whether the person has opened this capability's irreversible band (ADR 0024).
    /// Closed by default — the un-undoable stays blocked until deliberately opened.
    fn is_opened(&self, id: &str) -> bool {
        self.opened.get(id).copied().unwrap_or(false)
    }

    /// Whether the person set this capability to **ask first** (on with user input).
    /// Off by default — a skill follows its band unless deliberately set to confirm.
    fn is_confirm(&self, id: &str) -> bool {
        self.confirm.get(id).copied().unwrap_or(false)
    }

    /// The stored settings for a capability (empty if none set).
    fn settings_for(&self, id: &str) -> CapabilitySettings {
        self.settings.get(id).cloned().unwrap_or_default()
    }
}

/// Whether every setting a capability declares has a value — i.e. it is set up.
fn settings_complete(info: &CapabilityInfo, settings: &CapabilitySettings) -> bool {
    info.settings
        .iter()
        .all(|s| settings.get(s.key).is_some_and(|v| !v.trim().is_empty()))
}

/// The deterministic classifier at the heart of the autonomy envelope
/// (ADR 0022/0024): given a skill's declared [`Reversibility`] band, reach, and
/// the person's envelope, what does policy do — [`Act`](Decision::Act) on its
/// own, [`Confirm`](Decision::Confirm) first, or [`Block`](Decision::Block)
/// outright? Never consults the model — the boundary is policy.
///
/// The kernel owns the envelope-independent posture
/// ([`Reversibility::default_decision`]); this function applies the person's levers
/// on top of it: `auto_external` can *narrow* an otherwise-autonomous read that
/// leaves the device, and `auto_consequential` can *widen* an outward-but-reversible
/// action to run on its own.
///
/// `opened_irreversible` is the person's per-capability escape hatch (ADR 0024): by
/// default the irreversible band is [`Block`](Decision::Block) — refused, not
/// offered, because a mistaken confirm is unrecoverable — but when the person has
/// deliberately opened this capability it becomes [`Confirm`](Decision::Confirm).
/// It never becomes [`Act`](Decision::Act): the un-undoable is confirmed every
/// time, never run autonomously.
fn classify(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    opened_irreversible: bool,
    confirm_each_use: bool,
) -> Decision {
    let base = match info.reversibility.default_decision() {
        // The un-undoable is refused outright — deny-by-default (ADR 0024) — unless
        // the person has opened this capability, and even then only to confirm-each-
        // use, never to autonomous.
        Decision::Block => {
            if opened_irreversible {
                Decision::Confirm
            } else {
                Decision::Block
            }
        }
        // Autonomous by default (Observe / Reversible), but a read that leaves the
        // device waits for confirmation if the person narrowed the envelope to keep
        // on-device actions in-hand.
        Decision::Act => {
            if info.reaches_external && !env.auto_external {
                Decision::Confirm
            } else {
                Decision::Act
            }
        }
        // Confirm by default (outward but reversible); autonomous only when the
        // person has widened the envelope to allow consequential actions.
        Decision::Confirm => {
            if env.auto_consequential {
                Decision::Act
            } else {
                Decision::Confirm
            }
        }
    };
    // Per-skill "ask first" (on with user input): never runs on its own — the butler
    // proposes and waits. Downgrades an autonomous verdict to Confirm, and turns an
    // allow-with-confirm into the same (it can't relax a hard Block, though).
    match base {
        Decision::Act if confirm_each_use => Decision::Confirm,
        other => other,
    }
}

/// Whether a skill may run on its own this turn — exactly when the deterministic
/// [`classify`] verdict is [`Act`](Decision::Act). An opened irreversible skill or one
/// set to ask-first is never autonomous (it confirms every time).
fn may_run_autonomously(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    opened_irreversible: bool,
    confirm_each_use: bool,
) -> bool {
    classify(info, env, opened_irreversible, confirm_each_use) == Decision::Act
}

impl CapabilityRunner for RegistryRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.capabilities
            .iter()
            .map(|c| {
                let info = c.info();
                crate::application::CapabilitySpec {
                    id: info.id.to_owned(),
                    description: info.description.to_owned(),
                    // Usable only if the code is ready, the person has it enabled, AND
                    // every required setting has a value (ADR 0021).
                    configured: info.configured
                        && self.is_enabled(info.id)
                        && settings_complete(&info, &self.settings_for(info.id)),
                    reversibility: info.reversibility,
                    autonomous: may_run_autonomously(
                        &info,
                        &self.envelope,
                        self.is_opened(info.id),
                        self.is_confirm(info.id),
                    ),
                    // Built-ins describe their inputs in the prompt, not a schema.
                    input_schema: None,
                }
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.capabilities
            .iter()
            .find(|c| c.info().id == id)
            .map(|c| {
                classify(
                    &c.info(),
                    &self.envelope,
                    self.is_opened(id),
                    self.is_confirm(id),
                )
            })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        if !self.is_enabled(id) {
            return Err(format!("the '{id}' skill is turned off"));
        }
        let cap = self
            .capabilities
            .iter()
            .find(|c| c.info().id == id)
            .ok_or_else(|| format!("no such skill '{id}'"))?;
        // Deny-by-default on the irreversible band (ADR 0024): the un-undoable is
        // blocked outright — never run, even on an explicit request — until the
        // person opens it per capability. Once opened it reaches this path only via
        // an explicit confirmation. The failure mode is "it refused," never "it did
        // something permanent." The classifier owns which band is blocked.
        if classify(
            &cap.info(),
            &self.envelope,
            self.is_opened(id),
            self.is_confirm(id),
        ) == Decision::Block
        {
            return Err(format!(
                "the '{id}' skill can't be undone, so Endora won't run it on its own — \
                 this band stays blocked until you open it for this skill"
            ));
        }
        // Data-loss tripwire: for a skill that leaves the device, refuse to send a
        // request that appears to carry a secret (ADR 0023). Fail closed.
        if cap.info().reaches_external {
            if let Some(kind) = scan_outbound_secret(input_json) {
                return Err(format!(
                    "refusing to send this to '{id}' — the request looks like it contains {kind}"
                ));
            }
        }
        let mut input: Value = serde_json::from_str(input_json.trim())
            .or_else(|_| Ok::<Value, serde_json::Error>(json!({})))
            .unwrap_or_else(|_| json!({}));
        // Query minimization: strip personal identifiers (email addresses) from a
        // request before it leaves the device (ADR 0023).
        if cap.info().reaches_external {
            redact_pii_in_value(&mut input);
        }
        let out = cap
            .invoke(&input, &self.settings_for(id))
            .map_err(|e| e.to_string())?;
        // Hand the butler readable text, not raw JSON — small models relay it far
        // more reliably (and won't miss headlines buried in a JSON array).
        Ok(cap.summarize(&out))
    }
}

// ---- MCP host: transport port, adapter, and the composite runner ----------
//
// An MCP server is a *source* of catalog tools (ADR 0021). Because a tool's id and
// description are discovered at runtime — not the `&'static` metadata the built-in
// `Capability` trait carries — the adapter implements the application-layer
// `CapabilityRunner` directly (whose `CapabilitySpec` is owned), rather than the
// built-in `Capability` trait. The application still speaks only to one
// `CapabilityRunner`; it never learns a tool came from MCP.

/// A tool exposed by a connected MCP server, discovered at connect time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolInfo {
    /// The tool's own name on its server (un-namespaced), e.g. `"create_event"`.
    pub name: String,
    /// The one-line description the server advertises for it.
    pub description: String,
    /// The tool's JSON-Schema for its input (`inputSchema` from `tools/list`), if the
    /// server provided one. The model needs this to know *how* to call the tool — the
    /// field names and which are required — otherwise it only knows the tool exists.
    pub input_schema: Option<serde_json::Value>,
}

/// The boundary to a single MCP server (ADR 0021). The concrete transport — a local
/// stdio subprocess, or a networked HTTP/SSE connection — lives behind this port, so
/// neither the adapter nor the policy layer ever speaks the protocol. Synchronous to
/// match [`CapabilityRunner::run`]; an async transport bridges to this at its edge.
pub trait McpClient: Send + Sync {
    /// Lists the server's tools (its handshake + `tools/list`).
    ///
    /// # Errors
    /// A human-readable message if the server can't be reached or replies badly.
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String>;

    /// Calls one tool by its (un-namespaced) name with JSON input, returning its
    /// result as text.
    ///
    /// # Errors
    /// A human-readable message if the call fails.
    fn call(&self, tool: &str, input_json: &str) -> Result<String, String>;
}

/// One connected server: its name, its transport, and the tools it advertised.
struct McpConnection {
    server: String,
    transport: Box<dyn McpClient>,
    tools: Vec<McpToolInfo>,
    /// The tool the person nominated as this server's state reader (ADR 0038). Empty
    /// when nobody has said, which means no read-back — the honest default.
    reader_tool: String,
}

/// A [`CapabilityRunner`] backed by connected **MCP servers** (ADR 0021). Each
/// server's tools appear in the catalog **namespaced** as `server.tool`, so two
/// servers can never collide on a name.
///
/// Safety posture (this slice): MCP tools are **never autonomous**, and their policy
/// [`Decision`] is [`Block`](Decision::Block) — deny-by-default, treated exactly as
/// the unclassified/irreversible band (ADR 0024). The butler can *see* a tool and
/// propose it, but policy refuses to run it until a later slice classifies tools and
/// lets the person open specific ones. [`run`](Self::run) itself routes faithfully to
/// the transport — its contract is that it is only ever called once policy cleared.
pub struct McpRunner {
    connections: Vec<McpConnection>,
}

impl McpRunner {
    /// Whether this tool is the **state reader** for its server — the one the person
    /// nominated (ADR 0038).
    ///
    /// The nomination comes from the person, never from the server: a server announcing
    /// "I only read" is not evidence of anything, and policy must not take an unvetted
    /// third party's word (ADR 0005). A server with no nomination has no reader, and
    /// everything on it stays deny-by-default.
    fn is_state_reader(&self, id: &str) -> bool {
        let Some((server, tool)) = id.split_once('.') else {
            return false;
        };
        self.connections
            .iter()
            .any(|c| c.server == server && !c.reader_tool.is_empty() && c.reader_tool == tool)
    }

    /// Connects to each `(server_name, transport)`, discovering its tools up front. A
    /// server whose `list_tools` fails is **skipped** — it contributes no tools rather
    /// than failing the whole runner, so one unhealthy server can't take down the host
    /// (ADR 0021).
    #[must_use]
    pub fn connect(servers: Vec<(String, Box<dyn McpClient>)>) -> Self {
        Self::connect_with_readers(
            servers
                .into_iter()
                .map(|(server, transport)| (server, transport, String::new()))
                .collect(),
        )
    }

    /// Connects, carrying each server's nominated **state reader** (ADR 0038) — the tool
    /// whose result is an observation and through which that server's actions are
    /// verified. An empty nomination means no read-back for that server.
    #[must_use]
    pub fn connect_with_readers(servers: Vec<(String, Box<dyn McpClient>, String)>) -> Self {
        let connections = servers
            .into_iter()
            .filter_map(
                |(server, transport, reader_tool)| match transport.list_tools() {
                    Ok(tools) => Some(McpConnection {
                        server,
                        transport,
                        reader_tool,
                        tools,
                    }),
                    Err(_) => None,
                },
            )
            .collect();
        Self { connections }
    }

    /// Resolves a namespaced `server.tool` id to its connection and tool.
    fn find(&self, id: &str) -> Option<(&McpConnection, &McpToolInfo)> {
        let (server, tool) = id.split_once('.')?;
        let conn = self.connections.iter().find(|c| c.server == server)?;
        let info = conn.tools.iter().find(|t| t.name == tool)?;
        Some((conn, info))
    }
}

/// Renders an MCP tool's JSON-Schema input into a compact one-line hint the model can
/// act on — the field names, their types, and which are required — without dumping the
/// whole schema into the prompt. `None` when there are no properties to describe (a
/// no-argument tool needs no hint).
fn compact_input_hint(schema: &serde_json::Value) -> Option<String> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    let mut fields: Vec<String> = props
        .iter()
        .map(|(name, spec)| {
            let ty = spec.get("type").and_then(serde_json::Value::as_str);
            match (ty, required.contains(name.as_str())) {
                (Some(ty), true) => format!("{name} ({ty}, required)"),
                (Some(ty), false) => format!("{name} ({ty})"),
                (None, true) => format!("{name} (required)"),
                (None, false) => name.clone(),
            }
        })
        .collect();
    fields.sort(); // stable order → a deterministic prompt
    Some(format!("call with input fields: {}", fields.join(", ")))
}

impl CapabilityRunner for McpRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        // The ids Endora treats as state readers, resolved once rather than per tool.
        let reader_ids: std::collections::HashSet<String> = self
            .connections
            .iter()
            .flat_map(|c| {
                c.tools
                    .iter()
                    .map(move |t| format!("{}.{}", c.server, t.name))
            })
            .filter(|id| self.is_state_reader(id))
            .collect();
        let reader_ids = &reader_ids;
        self.connections
            .iter()
            .flat_map(|c| {
                c.tools.iter().map(move |t| {
                    // Fold the tool's input shape into the description so the model
                    // learns HOW to call it (field names + which are required), not
                    // just that it exists. Built-in skills get input examples in the
                    // prompt; MCP tools only have this schema to go on.
                    let description = match t.input_schema.as_ref().and_then(compact_input_hint) {
                        Some(hint) => format!("{} — {hint}", t.description),
                        None => t.description.clone(),
                    };
                    // A tool Endora itself designates as that server's state reader is
                    // a READ: it reports the world, it does not act on it. Left in the
                    // irreversible band it was handed `[unverified] this is what the
                    // tool reported about its own work` — telling the model not to trust
                    // the very reading that is supposed to be the confirmation, which is
                    // incoherent and was observed live. A read has no work of its own to
                    // be unverified about (ADR 0034/0037).
                    let reads_state =
                        reader_ids.contains(format!("{}.{}", c.server, t.name).as_str());
                    crate::application::CapabilitySpec {
                        id: format!("{}.{}", c.server, t.name),
                        description,
                        configured: true,
                        // Deny-by-default everywhere else: the server tells us nothing
                        // about whether a tool reads or actuates, so its result is a
                        // receipt, not evidence (ADR 0034).
                        autonomous: reads_state,
                        reversibility: if reads_state {
                            Reversibility::Observe
                        } else {
                            Reversibility::Irreversible
                        },
                        // The real schema travels structurally too (as text), so the
                        // model layer can offer this tool through native tool-calling.
                        input_schema: t.input_schema.as_ref().map(ToString::to_string),
                    }
                })
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        // Only answer for tools we host; an unknown id is `None` so a composite can
        // consult another source. Deny-by-default, except the state reader Endora uses
        // to verify actions — a read is in the `Observe` band and may run on its own, so
        // the read-back still works when the person has opened the actions but not the
        // reader.
        self.find(id).map(|_| {
            if self.is_state_reader(id) {
                Reversibility::Observe.default_decision()
            } else {
                Reversibility::Irreversible.default_decision()
            }
        })
    }

    /// Home Assistant hosts both actuators (`Hass*`) and a state reader
    /// (`GetLiveContext`) on the same server, so an action can be checked against the
    /// world it just claimed to change (ADR 0034). One mapping for the whole
    /// integration — every `Hass*` action verifies through the same reader.
    ///
    /// Servers Endora knows nothing about return `None`, so their results stay marked
    /// unverified rather than being vouched for.
    fn verifier(&self, id: &str) -> Option<String> {
        let (server, _) = id.split_once('.')?;
        // Whatever the person nominated for THIS server (ADR 0038). No integration is
        // named here: a calendar or filesystem server gets read-back on exactly the same
        // terms as Home Assistant, as soon as someone says which of its tools reads.
        let conn = self
            .connections
            .iter()
            .find(|c| c.server == server && !c.reader_tool.is_empty())?;
        let reader = format!("{server}.{}", conn.reader_tool);
        // Only if that server really exposes it, and never verify a read with itself.
        (id != reader && self.find(&reader).is_some()).then_some(reader)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        const WHOLE: &str = "{}";
        let Some(reader_id) = self.verifier(action_id) else {
            return WHOLE.to_owned();
        };
        let Some((_, reader)) = self.find(&reader_id) else {
            return WHOLE.to_owned();
        };
        // Whatever targeting arguments the READER also accepts, taken from the action's
        // own input. Schema against schema — nothing here knows what an "area" is.
        let (Some(schema), Ok(args)) = (
            reader.input_schema.as_ref(),
            serde_json::from_str::<Value>(action_input),
        ) else {
            return WHOLE.to_owned();
        };
        let (Some(props), Some(args)) = (
            schema.get("properties").and_then(Value::as_object),
            args.as_object(),
        ) else {
            return WHOLE.to_owned();
        };
        // Location narrows the reading; a KIND filter must not. Asked to turn off the
        // kitchen main — which is a `switch` — the model sent `domain: ["light"]`. If
        // the read-back inherited that, it would show the kitchen's lights and hide the
        // switch: exactly the entity that explains the failure. After an action that did
        // not land, "what is actually there" is the whole point of looking (ADR 0034).
        //
        // The split is by JSON shape rather than by field name, so it needs no knowledge
        // of any server: a scalar (`name`, `area`, `floor`) points at something, while an
        // array (`domain`, `device_class`) restricts which kinds count.
        let scoped: serde_json::Map<String, Value> = args
            .iter()
            .filter(|(key, value)| {
                props.contains_key(*key) && !value.is_null() && !value.is_array()
            })
            .map(|(key, value)| ((*key).clone(), (*value).clone()))
            .collect();
        if scoped.is_empty() {
            return WHOLE.to_owned();
        }
        Value::Object(scoped).to_string()
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let (conn, tool) = self
            .find(id)
            .ok_or_else(|| format!("no such MCP tool '{id}'"))?;
        // Weak models pass a scalar where the schema wants an array (e.g. HA's
        // HassTurnOn wants domain:["light"] but the model sends "light"). Coerce the
        // arguments to the tool's schema before the call so a single-value slip
        // doesn't fail validation.
        let args = coerce_args_to_schema(input_json, tool.input_schema.as_ref());
        let args = drop_domain_word_name(&args, &tool.name);
        reject_no_op_light_set(&args, &tool.name)?;
        conn.transport.call(&tool.name, &args)
    }
}

/// Nudges tool arguments toward the tool's input schema for the common small-model
/// slip of passing a scalar where an array is wanted: for each top-level property the
/// schema types as `array`, a non-array value is wrapped in a one-element array. Only
/// that one coercion — everything else is passed through untouched, and any parse
/// failure returns the input unchanged (the server still validates).
fn coerce_args_to_schema(input_json: &str, schema: Option<&serde_json::Value>) -> String {
    let Some(schema) = schema else {
        return input_json.to_owned();
    };
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return input_json.to_owned();
    };
    let Ok(mut args) = serde_json::from_str::<serde_json::Value>(input_json) else {
        return input_json.to_owned();
    };
    let Some(obj) = args.as_object_mut() else {
        return input_json.to_owned();
    };
    for (key, val) in obj.iter_mut() {
        let wants_array = props
            .get(key)
            .and_then(|s| s.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("array");
        if wants_array && !val.is_array() && !val.is_null() {
            *val = serde_json::Value::Array(vec![val.take()]);
        }
        // Drop values the schema says are not allowed, rather than letting the server
        // reject the whole call. Observed live: `device_class: ["light"]` — not one of
        // that field's permitted values — failed the entire turn with a validation
        // error, so a light nobody could name stayed on.
        if let Some(allowed) = permitted_values(props.get(key)) {
            match val {
                serde_json::Value::Array(items) => {
                    items.retain(|i| i.as_str().is_some_and(|s| allowed.iter().any(|a| a == s)));
                }
                serde_json::Value::String(sv) if !allowed.iter().any(|a| a == sv) => {
                    *val = serde_json::Value::Null;
                }
                _ => {}
            }
        }
    }
    // An empty value is not a filter, it is noise — and some servers reject it outright
    // (`floor: ""` came back as "invalid slot info", failing the call). Dropping empties
    // also clears anything emptied by the enum check above.
    obj.retain(|_, v| match v {
        serde_json::Value::Null => false,
        serde_json::Value::String(s) => !s.trim().is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        _ => true,
    });
    args.to_string()
}

/// The values a schema permits for a field, from `enum` — directly, or on an array's
/// `items`. `None` when the field is unconstrained, which is the common case.
///
/// Schema-driven on purpose: this is what lets Endora keep a model's slips from failing
/// a whole call without knowing anything about the server it is talking to (ADR 0038).
fn permitted_values(field: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let field = field?;
    let list = field
        .get("enum")
        .or_else(|| field.get("items").and_then(|i| i.get("enum")))?;
    let values: Vec<String> = list
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
        .collect();
    (!values.is_empty()).then_some(values)
}

/// Refuses a `HassLightSet` call that cannot do anything.
///
/// `HassLightSet` sets **brightness or colour**. Asked to turn a light on or off, a
/// model reaches for it anyway — and Home Assistant matches the targets, changes
/// nothing, and answers `action_done` with an empty `failed` list. The turn then
/// reports a success that never happened.
///
/// Observed live: "Please turn the kitchen light off" → `HassLightSet{area:"kitchen"}`
/// → HA reported success on three targets → the butler announced the device "has been
/// successfully adjusted" → **the light was still on.** The correct tools,
/// `HassTurnOn` and `HassTurnOff`, were available and accurately described the whole
/// time; the model simply picked the wrong one.
///
/// A false success is the worst failure this system can produce: it is unfalsifiable
/// from the conversation, and no amount of grounding helps a model whose tool told it
/// everything went fine. So the no-op is refused *before* it is sent, and the error
/// names the tool that would work — which is a fact about the API, not a canned
/// narration (ADR 0028): the butler still writes whatever it says next.
fn reject_no_op_light_set(args_json: &str, tool_name: &str) -> Result<(), String> {
    if tool_name != "HassLightSet" {
        return Ok(());
    }
    let Ok(args) = serde_json::from_str::<Value>(args_json) else {
        return Ok(());
    };
    let sets_something = ["brightness", "color", "temperature"]
        .iter()
        .any(|k| args.get(*k).is_some_and(|v| !v.is_null()));
    if sets_something {
        return Ok(());
    }
    Err(
        "HassLightSet only changes brightness or colour, and none were given, so this \
         would have done nothing. To switch a light on or off use HassTurnOn or \
         HassTurnOff instead."
            .to_owned(),
    )
}

/// Words that name a *kind* of device rather than a device. Home Assistant's intent
/// matcher reads `name` as a friendly name, so these never match anything.
const DEVICE_KIND_WORDS: &[&str] = &[
    "light",
    "lights",
    "lamp",
    "lamps",
    "switch",
    "switches",
    "fan",
    "fans",
    "cover",
    "covers",
    "blind",
    "blinds",
    "lock",
    "locks",
    "speaker",
    "speakers",
    "media player",
    "media players",
    "tv",
    "television",
    "thermostat",
    "thermostats",
    "everything",
    "all",
];

/// Drops a `name` that is a device *kind* rather than a device, when an `area` is
/// present.
///
/// Home Assistant matches `name` against an entity's friendly name. A small model
/// asked to "turn on the kitchen light" fills in `name:"light", area:"kitchen"`,
/// which asks HA for a device literally called "light" in the kitchen — it matches
/// nothing and comes back `MatchFailedError(NAME)`. Observed repeatedly against a
/// live install; the model picks the right tool and the right area every time and
/// only fumbles this one field.
///
/// The area alone expresses what the person meant, so the kind word is dropped.
/// **Only when an area is present**: without one, dropping the name would widen
/// "turn on the lights" to the whole house, so it is left to fail honestly rather
/// than act more broadly than asked (ADR 0024 — never act beyond the request).
///
/// Scoped to Home Assistant's `Hass*` tools; other MCP servers are passed through
/// untouched. This is the same "deterministic over prompting" move as the schema
/// coercion above: the slip is a known model failure, so the correction lives in
/// code rather than in a sterner instruction the model may ignore.
fn drop_domain_word_name(args_json: &str, tool_name: &str) -> String {
    if !tool_name.starts_with("Hass") {
        return args_json.to_owned();
    }
    let Ok(mut args) = serde_json::from_str::<serde_json::Value>(args_json) else {
        return args_json.to_owned();
    };
    let Some(obj) = args.as_object_mut() else {
        return args_json.to_owned();
    };
    let has_area = ["area", "area_name", "floor", "floor_name"]
        .iter()
        .any(|k| obj.get(*k).is_some_and(|v| !v.is_null()));
    if !has_area {
        return args_json.to_owned();
    }
    let is_kind_word = obj
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|n| {
            let n = n.trim().to_lowercase();
            DEVICE_KIND_WORDS.contains(&n.as_str())
        });
    if is_kind_word {
        obj.remove("name");
    }
    args.to_string()
}

/// Merges several [`CapabilityRunner`] sources — the built-in registry and any MCP
/// servers — behind the single runner interface (ADR 0021). The application still
/// speaks to one `CapabilityRunner` and never learns a tool's origin. Ids are unique
/// across sources by construction (built-ins carry no dot; MCP tools are
/// `server.tool`), so the first source that lists an id owns it.
pub struct CompositeRunner {
    sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>,
}

impl CompositeRunner {
    /// Merges the given sources, consulted in order. Sources are [`Arc`] so a
    /// long-lived one (e.g. connected MCP servers) can be shared across turns while a
    /// fresh per-turn source (the config-bound registry runner) sits beside it.
    #[must_use]
    pub fn new(sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>) -> Self {
        Self { sources }
    }

    /// The source that lists `id` in its catalog, if any.
    fn owner(&self, id: &str) -> Option<&(dyn CapabilityRunner + Send + Sync)> {
        self.sources
            .iter()
            .find(|s| s.available().iter().any(|spec| spec.id == id))
            .map(AsRef::as_ref)
    }
}

impl CapabilityRunner for CompositeRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.sources.iter().flat_map(|s| s.available()).collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.owner(id)?.decision(id)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        match self.owner(id) {
            Some(source) => source.run(id, input_json),
            None => Err(format!("no such skill '{id}'")),
        }
    }

    fn verifier(&self, id: &str) -> Option<String> {
        self.owner(id)?.verifier(id)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        self.owner(action_id).map_or_else(
            || "{}".to_owned(),
            |o| o.read_back_input(action_id, action_input),
        )
    }
}

/// A per-turn overlay that lifts an inner source's deny-by-default for tools the
/// person has **opened** (ADR 0024). An opened tool moves from
/// [`Block`](Decision::Block) to [`Confirm`](Decision::Confirm) — confirm-each-use —
/// and only opened tools may run; everything the person hasn't opened stays blocked.
///
/// When the person has *also* widened the autonomy envelope to act on consequential
/// things on its own (`auto_consequential`), an opened tool goes one step further to
/// [`Act`](Decision::Act): they've made two deliberate choices — allow this specific
/// tool, and allow acting without a per-use prompt — so the butler may run it in the
/// loop. (An ADR 0024 amendment: the un-undoable can become autonomous, but only
/// behind both of those explicit gates.) Wraps the shared MCP runner so specific MCP
/// tools can be allowed without rebuilding the connection.
pub struct OpenerRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    opened: std::collections::HashSet<String>,
    /// The person allowed acting on consequential things on its own — so an opened
    /// tool may run in the loop rather than only confirm-each-use.
    auto_consequential: bool,
}

impl OpenerRunner {
    /// Overlays `opened` (the ids the person has opened) onto `inner`. `auto_consequential`
    /// mirrors the autonomy envelope: with it on, opened tools may run autonomously.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        opened: std::collections::HashSet<String>,
        auto_consequential: bool,
    ) -> Self {
        Self {
            inner,
            opened,
            auto_consequential,
        }
    }
}

impl CapabilityRunner for OpenerRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                // An opened tool may run on its own only when the person also allowed
                // acting on consequential things autonomously; otherwise it confirms.
                if self.opened.contains(&spec.id) && self.auto_consequential {
                    spec.autonomous = true;
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        match self.inner.decision(id)? {
            // Opened: the un-undoable becomes confirm-each-use — or, when the person
            // allowed acting on its own, autonomous (both gates opened deliberately).
            Decision::Block if self.opened.contains(id) => Some(if self.auto_consequential {
                Decision::Act
            } else {
                Decision::Confirm
            }),
            other => Some(other),
        }
    }

    fn verifier(&self, id: &str) -> Option<String> {
        self.inner.verifier(id)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        self.inner.read_back_input(action_id, action_input)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // Deny-by-default at the run layer too: a blocked-and-unopened tool never
        // runs, even on a direct call. Opened tools (now confirm-each-use) run —
        // reaching run means policy cleared them (a confirmation happened).
        if self.inner.decision(id) == Some(Decision::Block) && !self.opened.contains(id) {
            return Err(format!(
                "'{id}' isn't allowed yet — open it under Skills first (it will still \
                 confirm every use)"
            ));
        }
        self.inner.run(id, input_json)
    }
}

/// A wrapper for the butler's **unattended** turns — the heartbeat's check-in, daily
/// brief, and nightly loop — that clamps autonomy to the *reversible* bands.
///
/// The person's levers ([`OpenerRunner`], the autonomy envelope) answer "may Endora do
/// this **when I am here**": opening a tool and widening the envelope together clear an
/// irreversible capability to run inside a chat turn, where the person is present, sees
/// the activity trail, and can say stop. None of that is true at 03:00 while they sleep.
///
/// So an unattended turn gets a narrower catalog: anything above
/// [`Reversibility::Reversible`] loses `autonomous` and its [`Act`](Decision::Act)
/// verdict drops to [`Confirm`](Decision::Confirm) — there is nobody to confirm, so in
/// practice it simply does not run, and the butler is handed a factual tool result
/// saying so. `Observe` and `Reversible` capabilities are untouched, which is what lets
/// the nightly loop still research, draft, and form beliefs (ADR 0024).
///
/// This makes [`run_due_nightly_loop`]'s documented guarantee — "nothing here it could
/// do that it couldn't undo" — true in code rather than only in prose. Before this, that
/// claim held only while the envelope happened to be closed.
///
/// Deny-by-default: a capability whose band cannot be read is assumed to be an actuator,
/// the same rule ADR 0034 applies to verification.
pub struct ReversibleOnlyRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
}

impl ReversibleOnlyRunner {
    /// Wraps `inner` so only reversible capabilities may act without a person.
    #[must_use]
    pub fn new(inner: Arc<dyn CapabilityRunner + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Whether this capability may run with nobody there — `Observe` and `Reversible`
    /// only. An id with no visible band is treated as an actuator, so it may not.
    fn may_run_unattended(&self, id: &str) -> bool {
        self.inner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .is_some_and(|s| s.reversibility <= Reversibility::Reversible)
    }
}

impl CapabilityRunner for ReversibleOnlyRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                if spec.reversibility > Reversibility::Reversible {
                    spec.autonomous = false;
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        match self.inner.decision(id)? {
            // Cleared to act when someone is present, but this is not that: fall back
            // to needing a person, who is by definition not here to give it.
            Decision::Act if !self.may_run_unattended(id) => Some(Decision::Confirm),
            other => Some(other),
        }
    }

    fn verifier(&self, id: &str) -> Option<String> {
        self.inner.verifier(id)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        self.inner.read_back_input(action_id, action_input)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // Refused at the run layer too, so a direct call can't route around the
        // narrowed catalog above.
        if !self.may_run_unattended(id) {
            return Err(format!(
                "'{id}' can't run unattended — it changes something Endora couldn't take \
                 back, and nobody is here to confirm it"
            ));
        }
        self.inner.run(id, input_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `CapabilityInfo` with a given band and reach; other fields are inert.
    fn info(reversibility: Reversibility, reaches_external: bool) -> CapabilityInfo {
        CapabilityInfo {
            id: "x",
            name: "X",
            description: "",
            category: "",
            reaches_external,
            reversibility,
            configured: true,
            needs: "",
            settings: &[],
        }
    }

    #[test]
    fn the_classifier_maps_band_reach_and_envelope_to_a_decision() {
        use crate::application::AutonomyEnvelope;
        use Reversibility::{OutwardReversible, Reversible};
        let default_env = AutonomyEnvelope::default(); // external ok, consequential no
        let no_external = AutonomyEnvelope {
            auto_external: false,
            auto_consequential: false,
        };
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };

        // `closed` = the irreversible opener is off; `ask` = the ask-first override.
        let closed = false;
        let ask = false;
        // Reversible local read: always acts.
        assert_eq!(
            classify(&info(Reversible, false), &default_env, closed, ask),
            Decision::Act
        );
        // Reversible external read: acts by default...
        assert_eq!(
            classify(&info(Reversible, true), &default_env, closed, ask),
            Decision::Act
        );
        // ...but waits for confirmation when the person narrows the envelope.
        assert_eq!(
            classify(&info(Reversible, true), &no_external, closed, ask),
            Decision::Confirm
        );
        // ...or when the person sets that skill to ask first (on with user input):
        // an otherwise-autonomous read now confirms every use.
        assert_eq!(
            classify(&info(Reversible, false), &default_env, closed, true),
            Decision::Confirm
        );
        // Outward but reversible: confirm by default, acts only when widened.
        assert_eq!(
            classify(&info(OutwardReversible, true), &default_env, closed, ask),
            Decision::Confirm
        );
        assert_eq!(
            classify(&info(OutwardReversible, true), &widened, closed, ask),
            Decision::Act
        );
        // Ask-first keeps a widened outward action confirming rather than acting.
        assert_eq!(
            classify(&info(OutwardReversible, true), &widened, closed, true),
            Decision::Confirm
        );

        // `may_run_autonomously` is exactly "the verdict is Act".
        assert!(may_run_autonomously(
            &info(Reversible, false),
            &default_env,
            closed,
            ask
        ));
        assert!(!may_run_autonomously(
            &info(OutwardReversible, true),
            &default_env,
            closed,
            ask
        ));
        // Ask-first is never autonomous.
        assert!(!may_run_autonomously(
            &info(Reversible, false),
            &default_env,
            closed,
            true
        ));
    }

    #[test]
    fn irreversible_is_blocked_when_closed_and_confirm_when_opened() {
        use crate::application::AutonomyEnvelope;
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };
        let irreversible = info(Reversibility::Irreversible, true);

        // Closed: blocked outright, not merely confirmed — even fully widened, and
        // even if set to ask-first (ask-first can't relax the hard irreversible Block).
        assert_eq!(
            classify(&irreversible, &widened, false, false),
            Decision::Block
        );
        assert_eq!(
            classify(&irreversible, &widened, false, true),
            Decision::Block
        );

        // Opened (ADR 0024 escape hatch): moves to Confirm — never Act. The
        // un-undoable is confirmed every time, never run autonomously, even fully
        // widened.
        assert_eq!(
            classify(&irreversible, &widened, true, false),
            Decision::Confirm
        );
        assert!(!may_run_autonomously(&irreversible, &widened, true, false));
    }

    #[test]
    fn run_refuses_an_irreversible_skill_deny_by_default() {
        // A skill whose effect can't be undone must be refused by the execution
        // path itself, not just excluded from autonomous runs (ADR 0024).
        struct IrreversibleSkill;
        impl Capability for IrreversibleSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "wire_transfer",
                    name: "Wire transfer",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                panic!("an irreversible skill must never be invoked");
            }
        }
        let runner = RegistryRunner::new(Arc::new(vec![
            Arc::new(IrreversibleSkill) as Arc<dyn Capability>
        ]));
        let err = runner.run("wire_transfer", "{}").unwrap_err();
        assert!(err.contains("can't be undone"), "unexpected error: {err}");
    }

    #[test]
    fn run_allows_an_opened_irreversible_skill_but_never_autonomously() {
        // Once the person opens a capability's irreversible band, the execution path
        // runs it (reached only via an explicit confirmation) — but it is still
        // never cleared to act on its own (ADR 0024).
        struct BookingSkill;
        impl Capability for BookingSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "booking",
                    name: "Booking",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Ok(json!({ "booked": true }))
            }
        }
        let caps: Arc<Vec<Arc<dyn Capability>>> =
            Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

        // Opened for this capability, fully-widened envelope.
        let runner = RegistryRunner::with_config(
            caps,
            vec![],
            vec![("booking".to_owned(), true)],
            vec![],
            crate::application::AutonomyEnvelope {
                auto_external: true,
                auto_consequential: true,
            },
            std::collections::HashMap::new(),
        );

        // It runs now (the person confirmed) — no longer blocked.
        assert_eq!(runner.run("booking", "{}").unwrap(), r#"{"booked":true}"#);
        // But it is never autonomous: it must be confirmed every time.
        let spec = runner
            .available()
            .into_iter()
            .find(|s| s.id == "booking")
            .unwrap();
        assert!(
            !spec.autonomous,
            "an opened irreversible skill must still confirm"
        );
    }

    #[test]
    fn decision_reports_the_full_verdict_including_block() {
        // `decision` exposes the real Act/Confirm/Block verdict (for the audit
        // trail), unlike the coarse `autonomous` bool which collapses the last two.
        struct BookingSkill;
        impl Capability for BookingSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "booking",
                    name: "Booking",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Ok(json!({}))
            }
        }
        let caps: Arc<Vec<Arc<dyn Capability>>> =
            Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

        // Closed: the un-undoable is Blocked (the bool would only say "not autonomous").
        let closed = RegistryRunner::new(caps.clone());
        assert_eq!(closed.decision("booking"), Some(Decision::Block));
        // Opened: Confirm — never Act.
        let opened = RegistryRunner::with_config(
            caps,
            vec![],
            vec![("booking".to_owned(), true)],
            vec![],
            crate::application::AutonomyEnvelope::default(),
            std::collections::HashMap::new(),
        );
        assert_eq!(opened.decision("booking"), Some(Decision::Confirm));
        // Unknown skill: no verdict.
        assert_eq!(closed.decision("nope"), None);
    }

    #[test]
    fn registry_has_modules_with_stable_ids() {
        let caps = default_capabilities();
        assert!(caps.len() >= 6);
        let ids: Vec<_> = caps.iter().map(|c| c.info().id).collect();
        assert!(ids.contains(&"weather"));
        assert!(ids.contains(&"web_fetch"));
        assert!(ids.contains(&"flights"));
    }

    #[test]
    fn a_scaffold_reports_unavailable_with_a_reason() {
        let err = FlightSearchCapability
            .invoke(&json!({}), &CapabilitySettings::new())
            .unwrap_err();
        assert!(matches!(err, CapabilityError::Unavailable(_)));
    }

    #[test]
    fn observed_time_formats_the_open_meteo_timestamp() {
        assert_eq!(
            observed_time("2026-07-23T07:00").as_deref(),
            Some("7:00 AM")
        );
        assert_eq!(
            observed_time("2026-07-23T00:15").as_deref(),
            Some("12:15 AM")
        );
        assert_eq!(
            observed_time("2026-07-23T13:30").as_deref(),
            Some("1:30 PM")
        );
        assert_eq!(
            observed_time("2026-07-23T12:00").as_deref(),
            Some("12:00 PM")
        );
        // Unrecognized shapes fall back to no tag (rather than a wrong one).
        assert_eq!(observed_time("garbage"), None);
        assert_eq!(observed_time("2026-07-23T99:00"), None);
    }

    #[test]
    fn geocode_candidates_simplify_city_state() {
        assert_eq!(
            geocode_candidates("New York NC"),
            vec!["New York NC", "New York"]
        );
        assert_eq!(
            geocode_candidates("New York, NC"),
            vec!["New York, NC", "New York"]
        );
        assert_eq!(geocode_candidates("Boston"), vec!["Boston"]);
        assert_eq!(geocode_candidates("San Francisco"), vec!["San Francisco"]);
    }

    #[test]
    fn zip_detector_ignores_non_zip_input_without_a_network_call() {
        // Non-5-digit or non-numeric input must fall through (Ok(None)), so a place
        // name never triggers the ZIP lookup.
        assert!(resolve_us_zip("New York").unwrap().is_none());
        assert!(resolve_us_zip("2827").unwrap().is_none());
        assert!(resolve_us_zip("abcde").unwrap().is_none());
        assert!(resolve_us_zip("Boston, MA").unwrap().is_none());
    }

    #[test]
    fn outbound_tripwire_flags_secrets_but_not_ordinary_text() {
        // Ordinary queries and URLs are NOT flagged (no false positives).
        assert_eq!(
            scan_outbound_secret("what's the weather in New York"),
            None
        );
        assert_eq!(
            scan_outbound_secret("https://example.com/articles/2026/summer-festival"),
            None
        );
        assert_eq!(scan_outbound_secret("{\"location\":\"10001\"}"), None);
        // Known credential shapes ARE flagged.
        assert_eq!(
            scan_outbound_secret("{\"q\":\"my key is sk-abcdefabcdefabcdefabcdef\"}"),
            Some("an API key")
        );
        assert_eq!(
            scan_outbound_secret("AKIAIOSFODNN7EXAMPLE and more"),
            Some("an AWS access key")
        );
        assert_eq!(
            scan_outbound_secret("ghp_1234567890abcdefghijklmnopqrstuv"),
            Some("a GitHub token")
        );
        assert!(scan_outbound_secret("-----BEGIN RSA PRIVATE KEY-----").is_some());
        // A secret embedded in a URL query string is caught too.
        assert_eq!(
            scan_outbound_secret("https://x.com/?token=sk-abcdef1234567890abcdef"),
            Some("an API key")
        );
        assert_eq!(
            scan_outbound_secret("token eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM"),
            Some("a token (JWT)")
        );
    }

    #[test]
    fn query_minimization_redacts_emails_but_not_urls_or_plain_text() {
        // A standalone email in a query is redacted.
        assert_eq!(
            redact_emails_in_text("email john.doe@example.com about the trip"),
            "email [redacted-email] about the trip"
        );
        // Trailing punctuation is handled; the rest of the word survives.
        assert_eq!(
            redact_emails_in_text("(contact a@b.com)"),
            "(contact [redacted-email])"
        );
        // A URL is one word and not an email — left intact.
        assert_eq!(
            redact_emails_in_text("https://site.com/path?x=1"),
            "https://site.com/path?x=1"
        );
        // Ordinary text is untouched.
        assert_eq!(
            redact_emails_in_text("weather in New York tomorrow"),
            "weather in New York tomorrow"
        );
        assert!(!looks_like_email("https://a.com/x@y"));
        assert!(looks_like_email("a@b.co"));
    }

    #[test]
    fn query_minimization_walks_json_values() {
        let mut v = json!({ "query": "reach me at me@x.org", "url": "https://ok.com/" });
        redact_pii_in_value(&mut v);
        assert_eq!(v["query"], json!("reach me at [redacted-email]"));
        assert_eq!(v["url"], json!("https://ok.com/")); // url untouched
    }

    #[test]
    fn egress_guard_blocks_internal_ip_literals() {
        // Loopback, RFC1918, link-local (incl. cloud metadata), and IPv6 loopback.
        for url in [
            "http://127.0.0.1/x",
            "http://192.168.1.14:8787/data",
            "http://10.0.0.5/",
            "http://169.254.169.254/latest/meta-data",
            "https://[::1]/",
            "http://[fd00::1]/",
        ] {
            assert!(
                matches!(guard_egress(url), Err(CapabilityError::BadInput(_))),
                "should have blocked {url}"
            );
        }
        // A non-http scheme is refused too.
        assert!(guard_egress("ftp://8.8.8.8/").is_err());
    }

    #[test]
    fn egress_guard_allows_public_ip_literals() {
        assert!(guard_egress("https://8.8.8.8/").is_ok());
        assert!(guard_egress("http://1.1.1.1:8080/x").is_ok());
    }

    #[test]
    fn host_and_port_parses_forms() {
        assert_eq!(
            host_and_port("https://example.com/path"),
            Some(("example.com".to_owned(), 443))
        );
        assert_eq!(
            host_and_port("http://example.com:8080/x?y=1"),
            Some(("example.com".to_owned(), 8080))
        );
        assert_eq!(
            host_and_port("http://user:pass@10.0.0.1/x"),
            Some(("10.0.0.1".to_owned(), 80))
        );
        assert_eq!(
            host_and_port("https://[::1]:9000/"),
            Some(("::1".to_owned(), 9000))
        );
    }

    #[test]
    fn redirect_resolution_handles_absolute_and_relative() {
        assert_eq!(
            resolve_redirect("https://a.com/x", "https://b.com/y"),
            "https://b.com/y"
        );
        assert_eq!(
            resolve_redirect("https://a.com/x/y", "/z"),
            "https://a.com/z"
        );
    }

    #[test]
    fn web_fetch_rejects_non_http() {
        let err = WebFetchCapability
            .invoke(
                &json!({ "url": "file:///etc/passwd" }),
                &CapabilitySettings::new(),
            )
            .unwrap_err();
        assert!(matches!(err, CapabilityError::BadInput(_)));
    }

    #[test]
    fn rss_items_are_extracted_with_titles_links_and_publisher() {
        let xml = "<rss><channel>\
            <item><title>Storms hit New York &amp; the region</title><link>https://ex.com/a</link>\
              <source url=\"https://www.wcnc.com\">WCNC</source></item>\
            <item><title><![CDATA[City council votes tonight]]></title></item>\
            <item><title>Third &#39;big&#39; story</title></item>\
            </channel></rss>";
        let items = extract_rss_items(xml, 6);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].0, "Storms hit New York & the region");
        assert_eq!(items[0].1, "https://ex.com/a"); // the source link is kept
        assert_eq!(items[0].2, "WCNC"); // publisher name from <source>
        assert_eq!(items[1].0, "City council votes tonight");
        assert_eq!(items[1].1, ""); // no link in the feed ⇒ empty, not fabricated
        assert_eq!(items[1].2, ""); // no <source> ⇒ empty, not fabricated
        assert_eq!(items[2].0, "Third 'big' story");
    }

    #[test]
    fn news_without_a_place_or_query_is_bad_input() {
        let err = LocalNewsCapability
            .invoke(&json!({}), &CapabilitySettings::new())
            .unwrap_err();
        assert!(matches!(err, CapabilityError::BadInput(_)));
    }

    #[test]
    fn knowledge_summarize_reads_the_extract_or_says_nothing_found() {
        let hit = json!({ "query": "Ada Lovelace", "found": true, "title": "Ada Lovelace",
            "extract": "Ada Lovelace was an English mathematician." });
        assert_eq!(
            KnowledgeCapability.summarize(&hit),
            "Ada Lovelace: Ada Lovelace was an English mathematician."
        );
        let miss = json!({ "query": "asdfqwer", "found": false, "extract": "" });
        assert!(
            KnowledgeCapability
                .summarize(&miss)
                .contains("couldn't find")
        );
    }

    #[test]
    fn web_answers_prefers_a_direct_answer_then_related() {
        let direct = json!({ "query": "capital of France", "answer": "Paris", "related": [] });
        assert_eq!(WebAnswersCapability.summarize(&direct), "Paris");
        let related = json!({ "query": "rust lang", "answer": "",
            "related": ["Rust is a systems language", "Memory safety without GC"] });
        let s = WebAnswersCapability.summarize(&related);
        assert!(s.contains("Rust is a systems language"));
        let empty = json!({ "query": "zzz", "answer": "", "related": [] });
        assert!(
            WebAnswersCapability
                .summarize(&empty)
                .contains("didn't find")
        );
    }

    #[test]
    fn home_assistant_needs_config_and_summarizes_state() {
        // Without url/token it's unavailable (never reaches the home).
        assert!(
            HomeAssistantCapability
                .invoke(&json!({}), &CapabilitySettings::new())
                .is_err()
        );
        // Summary reads entities as name: state.
        let out = json!({ "domain": "light", "count": 2, "entities": [
            { "entity": "light.kitchen", "name": "Kitchen", "state": "on" },
            { "entity": "light.desk", "name": "Desk", "state": "off" },
        ] });
        let s = HomeAssistantCapability.summarize(&out);
        assert!(s.contains("Kitchen: on"));
        assert!(s.contains("Desk: off"));
    }

    #[test]
    fn news_summarize_gives_a_readable_numbered_list() {
        let out = json!({
            "query": "New York news",
            "count": 2,
            "headlines": ["Council meets tonight", "Road closures downtown"],
            "note": "",
        });
        let text = LocalNewsCapability.summarize(&out);
        assert!(text.contains("New York news"));
        assert!(text.contains("1. Council meets tonight"));
        assert!(text.contains("2. Road closures downtown"));
        // Empty results read as a plain "none found", never as raw JSON.
        let empty = json!({ "query": "Nowhere news", "count": 0, "headlines": [] });
        assert!(
            LocalNewsCapability
                .summarize(&empty)
                .contains("No recent news")
        );
    }

    #[test]
    fn strip_html_drops_tags_and_scripts() {
        let t = strip_html("<html><script>bad()</script><p>Hello <b>world</b></p></html>");
        assert_eq!(t, "Hello world");
    }

    #[test]
    fn image_review_declares_a_model_setting_and_needs_it() {
        let cap = ImageReviewCapability {
            ollama_base: "http://localhost:11434".to_owned(),
        };
        // It declares a required "model" setting...
        assert_eq!(cap.info().settings.len(), 1);
        assert_eq!(cap.info().settings[0].key, "model");
        // ...and without it, running fails (never reaches the vision call).
        assert!(
            cap.invoke(
                &json!({ "image_url": "http://x/y.png" }),
                &CapabilitySettings::new()
            )
            .is_err()
        );
    }

    #[test]
    fn settings_complete_requires_every_declared_setting() {
        let cap = ImageReviewCapability {
            ollama_base: "http://localhost:11434".to_owned(),
        };
        let info = cap.info();
        assert!(!settings_complete(&info, &CapabilitySettings::new()));
        let mut s = CapabilitySettings::new();
        s.insert("model".to_owned(), "moondream".to_owned());
        assert!(settings_complete(&info, &s));
        // A keyless skill is always complete.
        assert!(settings_complete(
            &WeatherCapability.info(),
            &CapabilitySettings::new()
        ));
    }

    // ---- MCP host: adapter + composite -------------------------------------

    use crate::application::CapabilitySpec;

    /// A stand-in MCP server. `call` echoes `tool(input)` so a test can prove which
    /// tool was reached; an unhealthy server fails `list_tools`. No interior
    /// mutability, so it stays `Send + Sync` for `Box<dyn McpClient>`.
    struct FakeTransport {
        tools: Vec<McpToolInfo>,
        healthy: bool,
    }

    impl McpClient for FakeTransport {
        fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
            if self.healthy {
                Ok(self.tools.clone())
            } else {
                Err("server down".to_owned())
            }
        }
        fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
            Ok(format!("{tool}({input_json})"))
        }
    }

    fn tool(name: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_owned(),
            description: format!("does {name}"),
            input_schema: None,
        }
    }

    /// A tool that advertises the arguments it takes, so schema-against-schema logic
    /// has something real to match.
    fn schema_tool(name: &str, fields: &[&str]) -> McpToolInfo {
        let props: serde_json::Map<String, Value> = fields
            .iter()
            .map(|f| ((*f).to_owned(), serde_json::json!({ "type": "string" })))
            .collect();
        McpToolInfo {
            name: name.to_owned(),
            description: format!("does {name}"),
            input_schema: Some(serde_json::json!({ "type": "object", "properties": props })),
        }
    }

    #[test]
    fn mcp_runner_namespaces_tools_is_deny_by_default_and_routes() {
        let transport = FakeTransport {
            tools: vec![tool("create_event"), tool("list_events")],
            healthy: true,
        };
        let runner = McpRunner::connect(vec![("calendar".to_owned(), Box::new(transport))]);

        // Tools appear namespaced, configured, and never autonomous.
        let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["calendar.create_event", "calendar.list_events"]);
        assert!(
            runner
                .available()
                .iter()
                .all(|s| s.configured && !s.autonomous)
        );

        // Deny-by-default: a hosted tool blocks; an unknown id is None.
        assert_eq!(
            runner.decision("calendar.create_event"),
            Some(Decision::Block)
        );
        assert_eq!(runner.decision("calendar.nope"), None);
        assert_eq!(runner.decision("weather"), None);

        // `run` routes faithfully to the transport (its contract: only called once
        // policy has cleared the tool).
        assert_eq!(
            runner.run("calendar.create_event", "{\"x\":1}").unwrap(),
            "create_event({\"x\":1})"
        );
        assert!(runner.run("calendar.nope", "{}").is_err());
    }

    #[test]
    fn coerce_wraps_a_scalar_where_the_schema_wants_an_array() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "array" },
                "name": { "type": "string" }
            }
        });
        // The classic HA slip: domain sent as a string, not an array.
        let out = coerce_args_to_schema(r#"{"domain":"light","name":"kitchen"}"#, Some(&schema));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["domain"], serde_json::json!(["light"]));
        // A non-array field is left alone; an already-array value is untouched.
        assert_eq!(v["name"], "kitchen");
        let already = coerce_args_to_schema(r#"{"domain":["light"]}"#, Some(&schema));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&already).unwrap()["domain"],
            serde_json::json!(["light"])
        );
        // No schema, or unparseable input → passed through unchanged.
        assert_eq!(coerce_args_to_schema("{bad", Some(&schema)), "{bad");
        assert_eq!(coerce_args_to_schema(r#"{"x":1}"#, None), r#"{"x":1}"#);
    }

    /// The failure this fixes, taken verbatim from a live install: the butler picked
    /// the right tool and the right area, then put the *kind* word in `name`, and
    /// Home Assistant came back `MatchFailedError(NAME)` because no device is
    /// actually called "light".
    #[test]
    fn a_device_kind_word_in_name_is_dropped_when_an_area_is_given() {
        for kind in ["light", "lights", "Lights", " lamp ", "switch"] {
            let args = format!(r#"{{"name":"{kind}","area":"kitchen"}}"#);
            let out = drop_domain_word_name(&args, "HassTurnOn");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert!(v.get("name").is_none(), "should have dropped name={kind:?}");
            assert_eq!(v["area"], "kitchen", "the area is what the person meant");
        }
    }

    /// The live failure: "Please turn the kitchen light off" produced
    /// `HassLightSet{area:"kitchen"}`. HA matched three targets, changed nothing,
    /// and answered `action_done` — so the butler announced success and the light
    /// stayed on.
    #[test]
    fn a_light_set_that_sets_nothing_is_refused_before_it_is_sent() {
        let err = reject_no_op_light_set(r#"{"area":"kitchen"}"#, "HassLightSet").unwrap_err();
        assert!(
            err.contains("HassTurnOff"),
            "names the tool that works: {err}"
        );
        assert!(err.contains("would have done nothing"), "got: {err}");
    }

    #[test]
    fn a_light_set_that_actually_sets_something_goes_through() {
        for args in [
            r#"{"area":"kitchen","brightness":50}"#,
            r#"{"area":"kitchen","color":"warm white"}"#,
            r#"{"name":"Kitchen Table","temperature":2700}"#,
        ] {
            assert!(
                reject_no_op_light_set(args, "HassLightSet").is_ok(),
                "should have been allowed: {args}"
            );
        }
    }

    #[test]
    fn the_refusal_is_scoped_to_light_set() {
        // Turning something off with no extra parameters is exactly right for these.
        assert!(reject_no_op_light_set(r#"{"area":"kitchen"}"#, "HassTurnOff").is_ok());
        assert!(reject_no_op_light_set(r#"{"area":"kitchen"}"#, "HassTurnOn").is_ok());
        assert!(reject_no_op_light_set(r#"{"q":"x"}"#, "search_notes").is_ok());
        // Unparseable input is passed on rather than guessed at.
        assert!(reject_no_op_light_set("{bad", "HassLightSet").is_ok());
    }

    #[test]
    fn a_null_brightness_does_not_count_as_setting_something() {
        let err = reject_no_op_light_set(r#"{"area":"kitchen","brightness":null}"#, "HassLightSet")
            .unwrap_err();
        assert!(err.contains("HassTurnOn"), "got: {err}");
    }

    #[test]
    fn a_real_device_name_is_never_dropped() {
        let out = drop_domain_word_name(r#"{"name":"reading lamp","area":"study"}"#, "HassTurnOn");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["name"], "reading lamp");
    }

    #[test]
    fn without_an_area_the_name_stands_and_the_call_fails_honestly() {
        // Dropping it here would widen "turn on the lights" to the whole house —
        // acting beyond what was asked. Better to let HA refuse the match.
        let args = r#"{"name":"lights"}"#;
        assert_eq!(drop_domain_word_name(args, "HassTurnOn"), args);
    }

    #[test]
    fn non_home_assistant_tools_are_passed_through_untouched() {
        let args = r#"{"name":"lights","area":"kitchen"}"#;
        assert_eq!(drop_domain_word_name(args, "search_notes"), args);
        // Unparseable input is passed through rather than guessed at.
        assert_eq!(drop_domain_word_name("{bad", "HassTurnOn"), "{bad");
    }

    #[test]
    fn compact_input_hint_lists_fields_types_and_required() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "area": { "type": "string" }
            },
            "required": ["name"]
        });
        let hint = compact_input_hint(&schema).unwrap();
        assert!(hint.contains("name (string, required)"), "got: {hint}");
        assert!(hint.contains("area (string)"), "got: {hint}");
        // A no-argument tool needs no hint.
        assert!(
            compact_input_hint(&serde_json::json!({ "type": "object", "properties": {} }))
                .is_none()
        );
        assert!(compact_input_hint(&serde_json::json!({})).is_none());
    }

    #[test]
    fn mcp_available_folds_the_input_schema_into_the_description() {
        let mut turn_on = tool("HassTurnOn");
        turn_on.description = "Turns on a device".to_owned();
        turn_on.input_schema = Some(serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        }));
        let transport = FakeTransport {
            tools: vec![turn_on],
            healthy: true,
        };
        let runner = McpRunner::connect(vec![("home".to_owned(), Box::new(transport))]);
        let spec = runner
            .available()
            .into_iter()
            .find(|s| s.id == "home.HassTurnOn")
            .unwrap();
        // The model sees both what the tool does AND how to call it.
        assert!(spec.description.contains("Turns on a device"));
        assert!(
            spec.description.contains("name (string, required)"),
            "description should carry the input hint, got: {}",
            spec.description
        );
    }

    #[test]
    fn mcp_runner_skips_an_unhealthy_server() {
        let ok = FakeTransport {
            tools: vec![tool("read_file")],
            healthy: true,
        };
        let down = FakeTransport {
            tools: vec![tool("send_mail")],
            healthy: false,
        };
        let runner = McpRunner::connect(vec![
            ("files".to_owned(), Box::new(ok)),
            ("mail".to_owned(), Box::new(down)),
        ]);
        // Only the healthy server contributes tools; the down one drops out silently.
        let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["files.read_file"]);
    }

    /// A minimal built-in-like source: an autonomous `weather` skill.
    struct FakeBuiltin;
    impl CapabilityRunner for FakeBuiltin {
        fn available(&self) -> Vec<CapabilitySpec> {
            vec![CapabilitySpec {
                id: "weather".to_owned(),
                description: "the weather".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: endora_kernel::Reversibility::Observe,
            }]
        }
        fn run(&self, id: &str, _input: &str) -> Result<String, String> {
            if id == "weather" {
                Ok("sunny".to_owned())
            } else {
                Err(format!("no such skill '{id}'"))
            }
        }
    }

    #[test]
    fn composite_runner_merges_sources_and_routes_by_owner() {
        let mcp = McpRunner::connect(vec![(
            "calendar".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("create_event")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]);
        let composite = CompositeRunner::new(vec![Arc::new(FakeBuiltin), Arc::new(mcp)]);

        // Both sources' catalogs are merged.
        let ids: Vec<String> = composite.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["weather", "calendar.create_event"]);

        // Each source keeps its own policy verdict.
        assert_eq!(composite.decision("weather"), Some(Decision::Act));
        assert_eq!(
            composite.decision("calendar.create_event"),
            Some(Decision::Block)
        );
        assert_eq!(composite.decision("missing"), None);

        // `run` dispatches to whichever source owns the id.
        assert_eq!(composite.run("weather", "{}").unwrap(), "sunny");
        assert_eq!(
            composite.run("calendar.create_event", "{}").unwrap(),
            "create_event({})"
        );
        assert!(composite.run("missing", "{}").is_err());
    }

    #[test]
    fn the_nominated_reader_is_an_observation_not_a_receipt() {
        // Observed live: GetLiveContext's own result came back stamped
        //   "[unverified] This is what the tool reported about its own work."
        // A read has no work of its own, and that text tells the model not to trust the
        // very reading ADR 0034 uses as confirmation.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let specs = mcp.available();
        let reader = specs
            .iter()
            .find(|s| s.id == "home-assistant.GetLiveContext")
            .expect("the reader is offered");
        assert_eq!(reader.reversibility, Reversibility::Observe);
        assert!(reader.autonomous, "a read may run on its own");
        assert_eq!(
            mcp.decision("home-assistant.GetLiveContext"),
            Some(Decision::Act)
        );

        // Everything else on the server stays deny-by-default, and is verified through
        // the nominated reader.
        let action = specs
            .iter()
            .find(|s| s.id == "home-assistant.HassTurnOff")
            .expect("the action is offered");
        assert_eq!(action.reversibility, Reversibility::Irreversible);
        assert!(!action.autonomous);
        assert_eq!(
            mcp.verifier("home-assistant.HassTurnOff").as_deref(),
            Some("home-assistant.GetLiveContext")
        );
        // A reader never verifies itself.
        assert_eq!(mcp.verifier("home-assistant.GetLiveContext"), None);
    }

    #[test]
    fn arguments_the_schema_forbids_are_dropped_not_sent() {
        // Live failure: device_class:["light"] — not one of that field's permitted
        // values — failed the WHOLE call with a validation error, so a light nobody
        // could name stayed on. The schema says which values are allowed; use it.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "floor": { "type": "string" },
                "device_class": {
                    "type": "array",
                    "items": { "enum": ["outlet", "switch", "tv"] }
                }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"name":"main","floor":"","device_class":["light"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["name"], "main");
        assert!(
            parsed.get("device_class").is_none(),
            "an invalid enum value was sent: {out}"
        );
        assert!(
            parsed.get("floor").is_none(),
            "an empty filter was sent: {out}"
        );
    }

    #[test]
    fn a_permitted_value_survives_the_hygiene() {
        // The check must not eat valid arguments — that would break every working call.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "device_class": {
                    "type": "array",
                    "items": { "enum": ["outlet", "switch"] }
                }
            }
        });
        let out = coerce_args_to_schema(r#"{"device_class":"switch"}"#, Some(&schema));
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["device_class"],
            serde_json::json!(["switch"]),
            "a valid value was coerced to an array and kept: {out}"
        );
    }

    #[test]
    fn the_read_back_is_scoped_to_what_the_action_targeted() {
        // The live failure: "turn the kitchen main off" read state back with no
        // arguments, got every device in the house, and the butler answered about the
        // GARAGE — the first thing in the dump that happened to be on.
        //
        // Home Assistant's GetLiveContext accepts name/domain/area filters, and the turn
        // was passing none of them.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "area", "domain"]),
                    schema_tool("GetLiveContext", &["name", "area", "domain"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"area":"kitchen","name":"main"}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["area"], "kitchen");
        assert_eq!(parsed["name"], "main");
    }

    #[test]
    fn a_kind_filter_never_narrows_the_read_back() {
        // The live case: "kitchen main" is a SWITCH, and the model sent
        // domain: ["light"]. Inheriting that would hide the one entity that explains
        // the failure — after an action that did not land, what is actually there is
        // the whole point of looking.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "area", "domain"]),
                    schema_tool("GetLiveContext", &["name", "area", "domain"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"area":"kitchen","name":"main","domain":["light"]}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["area"], "kitchen", "location still narrows");
        assert!(
            parsed.get("domain").is_none(),
            "the kind filter leaked into the reading: {scoped}"
        );
    }

    #[test]
    fn a_reader_only_gets_arguments_it_understands() {
        // Schema against schema: an argument the reader does not accept must not be
        // forwarded, or the read-back fails and the action goes unverified.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "brightness"]),
                    schema_tool("GetLiveContext", &["name"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"name":"kitchen","brightness":40}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["name"], "kitchen");
        assert!(
            parsed.get("brightness").is_none(),
            "an argument the reader can't take was forwarded: {scoped}"
        );
    }

    #[test]
    fn an_untargeted_action_still_reads_the_whole_state() {
        // No overlap, no scoping — the whole reading, which is the old behaviour and
        // the right fallback.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name"]),
                    schema_tool("GetLiveContext", &["name"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);
        assert_eq!(
            mcp.read_back_input("home-assistant.HassTurnOff", "{}"),
            "{}"
        );
    }

    #[test]
    fn any_server_gets_read_back_once_someone_nominates_its_reader() {
        // ADR 0038's whole point. Nothing here is Home Assistant, and nothing in the
        // runner knows what this server is — a calendar gets verification on exactly
        // the same terms, because the mapping is data rather than a name in the source.
        let mcp = McpRunner::connect_with_readers(vec![(
            "calendar".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("create_event"), tool("list_events")],
                healthy: true,
            }) as Box<dyn McpClient>,
            "list_events".to_owned(),
        )]);

        assert_eq!(
            mcp.verifier("calendar.create_event").as_deref(),
            Some("calendar.list_events"),
            "an action is verified through the nominated reader"
        );
        let reader = mcp
            .available()
            .into_iter()
            .find(|s| s.id == "calendar.list_events")
            .expect("offered");
        assert_eq!(reader.reversibility, Reversibility::Observe);
        assert!(reader.autonomous);
    }

    #[test]
    fn a_server_with_no_nomination_gets_no_reader_and_no_read_back() {
        // The honest default, unchanged: Endora does not guess which tool reads, and a
        // server's own say-so is not evidence (ADR 0005/0038). Note this holds even for
        // a tool literally named GetLiveContext — the old hardcode is gone.
        let mcp = McpRunner::connect(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]);
        assert_eq!(mcp.verifier("home-assistant.HassTurnOff"), None);
        for spec in mcp.available() {
            assert_eq!(spec.reversibility, Reversibility::Irreversible);
            assert!(!spec.autonomous);
        }
    }

    #[test]
    fn opener_runner_lifts_block_to_confirm_only_for_opened_tools() {
        let mcp = Arc::new(McpRunner::connect(vec![(
            "fs".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("read_file"), tool("write_file")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        // The person has opened only fs.write_file, but has NOT allowed acting on its
        // own (auto_consequential = false).
        let opened: std::collections::HashSet<String> = ["fs.write_file".to_owned()].into();
        let overlay = OpenerRunner::new(mcp, opened, false);

        // Deny-by-default holds for the un-opened tool; the opened one becomes
        // confirm-each-use (never autonomous — its spec stays non-autonomous).
        assert_eq!(overlay.decision("fs.read_file"), Some(Decision::Block));
        assert_eq!(overlay.decision("fs.write_file"), Some(Decision::Confirm));
        assert!(overlay.available().iter().all(|s| !s.autonomous));

        // Only the opened tool may run; the blocked one is refused even on a direct
        // call. The opened tool routes through to the transport.
        assert!(overlay.run("fs.read_file", "{}").is_err());
        assert_eq!(
            overlay.run("fs.write_file", "{\"p\":1}").unwrap(),
            "write_file({\"p\":1})"
        );
    }

    #[test]
    fn unattended_runner_keeps_an_opened_actuator_from_running_with_nobody_there() {
        // The exact configuration the nightly loop's safety claim has to survive: the
        // person opened a Home Assistant actuator AND widened the envelope, so in a
        // chat turn `OpenerRunner` clears it to act on its own. Unattended, it must not.
        let mcp = Arc::new(McpRunner::connect(vec![(
            "home".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        let opened: std::collections::HashSet<String> = ["home.HassTurnOff".to_owned()].into();
        let attended = Arc::new(OpenerRunner::new(mcp, opened, true));
        // Precondition: attended, it really is cleared to act.
        assert_eq!(attended.decision("home.HassTurnOff"), Some(Decision::Act));

        let unattended = ReversibleOnlyRunner::new(attended);
        // Unattended, the same tool falls back to needing a person.
        assert_eq!(
            unattended.decision("home.HassTurnOff"),
            Some(Decision::Confirm)
        );
        assert!(unattended.available().iter().all(|s| !s.autonomous));
        // And it is refused at the run layer too, not merely un-offered.
        assert!(unattended.run("home.HassTurnOff", "{}").is_err());
    }

    #[test]
    fn unattended_runner_leaves_reversible_skills_alone() {
        // The nightly loop still has to be able to research: an `Observe`-band skill
        // keeps acting on its own, which is the whole point of the loop.
        let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
        assert_eq!(unattended.decision("weather"), Some(Decision::Act));
        assert!(unattended.available().iter().all(|s| s.autonomous));
        assert_eq!(unattended.run("weather", "{}").unwrap(), "sunny");
    }

    #[test]
    fn unattended_runner_treats_an_unknown_band_as_an_actuator() {
        // Deny-by-default, same rule ADR 0034 applies to verification: a capability
        // whose band we cannot see is assumed to change something.
        let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
        assert!(unattended.run("mystery", "{}").is_err());
    }

    #[test]
    fn opener_runner_lets_opened_tools_act_when_autonomy_is_widened() {
        let mcp = Arc::new(McpRunner::connect(vec![(
            "home".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOn"), tool("HassTurnOff")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        let opened: std::collections::HashSet<String> = ["home.HassTurnOn".to_owned()].into();
        // Both gates open: this specific tool is opened AND the person allowed acting
        // on consequential things on its own.
        let overlay = OpenerRunner::new(mcp, opened, true);

        // The opened tool may now run in the loop (Act + autonomous); the un-opened
        // one is still blocked, and never autonomous.
        assert_eq!(overlay.decision("home.HassTurnOn"), Some(Decision::Act));
        assert_eq!(overlay.decision("home.HassTurnOff"), Some(Decision::Block));
        let specs = overlay.available();
        assert!(
            specs
                .iter()
                .find(|s| s.id == "home.HassTurnOn")
                .unwrap()
                .autonomous
        );
        assert!(
            !specs
                .iter()
                .find(|s| s.id == "home.HassTurnOff")
                .unwrap()
                .autonomous
        );
    }
}
