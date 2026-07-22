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
use endora_kernel::{AutonomyLevel, Decision, Reversibility};
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
    /// Whether its effect can be undone (read-only, a draft, a deletable log) vs.
    /// permanent (sending, spending, editing/deleting external state). The autonomy
    /// classifier NEVER runs an irreversible skill on its own (ADR 0024).
    pub reversible: bool,
    /// May it act on its own (read-only/low-stakes), or must it ask first?
    pub autonomy: AutonomyLevel,
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
    // A bare US ZIP ("28277") — common when the person types their postcode — is
    // not resolvable by the place-name geocoder, so use a keyless ZIP lookup.
    if let Some(point) = resolve_us_zip(q)? {
        return Ok(point);
    }
    // The Open-Meteo geocoder wants a bare city name — it returns nothing for
    // "Charlotte NC" or "Charlotte, NC". So try the full query, then simpler forms
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
/// "Charlotte NC" / "Charlotte, NC" both fall back to "Charlotte").
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
        if let (Some(hi), Some(lo)) = (o["high_c"].as_f64(), o["low_c"].as_f64()) {
            s.push_str(&format!("; high {}, low {} today", cf(hi), cf(lo)));
        }
        if let Some(w) = o["warning"].as_str().filter(|w| !w.is_empty()) {
            s.push_str(&format!(". {w}"));
        }
        s
    }
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
        // place name first ("28277" → "Charlotte, NC news"). One of query/location
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
    ($ty:ident, $id:literal, $name:literal, $desc:literal, $cat:literal, $external:literal, $reversible:literal, $auto:expr, $needs:literal) => {
        struct $ty;
        impl Capability for $ty {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: $id,
                    name: $name,
                    description: $desc,
                    category: $cat,
                    reaches_external: $external,
                    reversible: $reversible,
                    autonomy: $auto,
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
    true, // reversible: read-only lookup
    AutonomyLevel::ActWithinPolicy,
    "an events data source / API key"
);
scaffold!(
    FlightSearchCapability,
    "flights",
    "Flight search",
    "Find and compare flights for a trip.",
    "travel",
    true,
    false, // irreversible: booking spends money and can't be undone
    AutonomyLevel::ConfirmEachAction,
    "a flights API key (booking stays a human decision)"
);
scaffold!(
    LocationLogCapability,
    "location",
    "Location tracking",
    "Keep a private log of where you are while travelling, so the butler has context.",
    "presence",
    false,
    true, // reversible: a private log you can delete
    AutonomyLevel::ConfirmEachAction,
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
    true, // reversible: read-only lookup
    AutonomyLevel::ActWithinPolicy,
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
            reversible: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
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
/// own this turn) exactly when its autonomy is [`AutonomyLevel::ActWithinPolicy`]
/// — read-only, low-stakes. Anything that must confirm stays gated.
pub struct RegistryRunner {
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// Per-capability enabled overrides (id → enabled). Missing = default enabled.
    enabled: std::collections::HashMap<String, bool>,
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
            envelope: crate::application::AutonomyEnvelope::default(),
            settings: std::collections::HashMap::new(),
        }
    }

    /// Wraps the registry, applying the person's enable/disable overrides (ADR 0021),
    /// their autonomy envelope (ADR 0022), and per-capability settings (ADR 0021). A
    /// disabled skill never runs; the envelope decides which kinds of action may run
    /// without confirmation; settings make a configurable skill usable.
    #[must_use]
    pub fn with_config(
        capabilities: Arc<Vec<Arc<dyn Capability>>>,
        overrides: Vec<(String, bool)>,
        envelope: crate::application::AutonomyEnvelope,
        settings: std::collections::HashMap<String, CapabilitySettings>,
    ) -> Self {
        Self {
            capabilities,
            enabled: overrides.into_iter().collect(),
            envelope,
            settings,
        }
    }

    /// Whether a capability is enabled (its override, or its built-in default).
    fn is_enabled(&self, id: &str) -> bool {
        self.enabled.get(id).copied().unwrap_or(true)
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

/// The **reversibility band** (ADR 0024) a capability's declared metadata places
/// it in — the primary axis of the autonomy envelope. Derived deterministically
/// from the metadata, never a model's say-so: a permanent effect is
/// [`Irreversible`](Reversibility::Irreversible); a pure reader is
/// [`Observe`](Reversibility::Observe); a low-stakes, undoable effect is
/// [`Reversible`](Reversibility::Reversible); and a consequential-but-undoable one
/// is [`OutwardReversible`](Reversibility::OutwardReversible).
fn reversibility_band(info: &CapabilityInfo) -> Reversibility {
    if !info.reversible {
        return Reversibility::Irreversible;
    }
    match info.autonomy {
        AutonomyLevel::Observe => Reversibility::Observe,
        AutonomyLevel::ActWithinPolicy => Reversibility::Reversible,
        AutonomyLevel::Suggest | AutonomyLevel::ConfirmEachAction => {
            Reversibility::OutwardReversible
        }
    }
}

/// The deterministic classifier at the heart of the autonomy envelope
/// (ADR 0022/0024): given a skill's reversibility band, reach, and the person's
/// envelope, what does policy do — [`Act`](Decision::Act) on its own,
/// [`Confirm`](Decision::Confirm) first, or [`Block`](Decision::Block) outright?
/// Never consults the model — the boundary is policy.
fn classify(info: &CapabilityInfo, env: &crate::application::AutonomyEnvelope) -> Decision {
    let band = reversibility_band(info);
    // The un-undoable is refused outright — deny-by-default, whatever the envelope
    // says (ADR 0024). It is not offered for confirmation: a mistaken confirm is
    // unrecoverable, so the band stays blocked until the person opens it per
    // capability (no opener exists yet). The kernel owns this posture.
    if band.default_decision() == Decision::Block {
        return Decision::Block;
    }
    // Within the reversible bands, autonomy + envelope decide whether it runs on
    // its own or waits for confirmation.
    match info.autonomy {
        // Observe-only skills never act on their own.
        AutonomyLevel::Observe => Decision::Confirm,
        // Read-only / low-stakes: autonomous, unless it leaves the device and the
        // person has narrowed the envelope to keep on-device actions in-hand.
        AutonomyLevel::ActWithinPolicy => {
            if !info.reaches_external || env.auto_external {
                Decision::Act
            } else {
                Decision::Confirm
            }
        }
        // Consequential but reversible: only autonomous if the person has widened
        // the envelope to allow it; otherwise it surfaces for confirmation.
        AutonomyLevel::Suggest | AutonomyLevel::ConfirmEachAction => {
            if env.auto_consequential {
                Decision::Act
            } else {
                Decision::Confirm
            }
        }
    }
}

/// Whether a skill may run on its own this turn — exactly when the deterministic
/// [`classify`] verdict is [`Act`](Decision::Act).
fn may_run_autonomously(info: &CapabilityInfo, env: &crate::application::AutonomyEnvelope) -> bool {
    classify(info, env) == Decision::Act
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
                    autonomous: may_run_autonomously(&info, &self.envelope),
                }
            })
            .collect()
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
        // person opens it per capability. The failure mode is "it refused," never
        // "it did something permanent." The classifier owns which band is blocked.
        if classify(&cap.info(), &self.envelope) == Decision::Block {
            return Err(format!(
                "the '{id}' skill can't be undone, so Endora won't run it on its own — \
                 this band stays blocked until you open it"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_classifier_gates_by_autonomy_and_reach() {
        use crate::application::AutonomyEnvelope;
        let info = |autonomy, reaches_external| CapabilityInfo {
            id: "x",
            name: "X",
            description: "",
            category: "",
            reaches_external,
            reversible: true,
            autonomy,
            configured: true,
            needs: "",
            settings: &[],
        };
        let default_env = AutonomyEnvelope::default(); // external ok, consequential no

        // Read-only local: always autonomous.
        assert!(may_run_autonomously(
            &info(AutonomyLevel::ActWithinPolicy, false),
            &default_env
        ));
        // Read-only external: autonomous by default...
        assert!(may_run_autonomously(
            &info(AutonomyLevel::ActWithinPolicy, true),
            &default_env
        ));
        // ...but not if the person narrows the envelope.
        let no_external = AutonomyEnvelope {
            auto_external: false,
            auto_consequential: false,
        };
        assert!(!may_run_autonomously(
            &info(AutonomyLevel::ActWithinPolicy, true),
            &no_external
        ));
        // Consequential: confirm by default, autonomous only when widened.
        assert!(!may_run_autonomously(
            &info(AutonomyLevel::ConfirmEachAction, true),
            &default_env
        ));
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };
        assert!(may_run_autonomously(
            &info(AutonomyLevel::ConfirmEachAction, true),
            &widened
        ));
        // Observe never acts, even fully widened.
        assert!(!may_run_autonomously(
            &info(AutonomyLevel::Observe, false),
            &widened
        ));

        // The un-undoable is NEVER autonomous, even fully widened (ADR 0024).
        let irreversible = CapabilityInfo {
            id: "book",
            name: "Book",
            description: "",
            category: "",
            reaches_external: true,
            reversible: false,
            autonomy: AutonomyLevel::ConfirmEachAction,
            configured: true,
            needs: "",
            settings: &[],
        };
        assert!(!may_run_autonomously(&irreversible, &widened));
    }

    #[test]
    fn metadata_maps_to_a_reversibility_band() {
        let info = |reversible, autonomy| CapabilityInfo {
            id: "x",
            name: "X",
            description: "",
            category: "",
            reaches_external: true,
            reversible,
            autonomy,
            configured: true,
            needs: "",
            settings: &[],
        };
        // A permanent effect is the un-undoable, whatever its autonomy level.
        assert_eq!(
            reversibility_band(&info(false, AutonomyLevel::ConfirmEachAction)),
            Reversibility::Irreversible
        );
        // A pure reader observes; a low-stakes undoable effect is the experiment
        // band; a consequential-but-undoable effect is outward-reversible.
        assert_eq!(
            reversibility_band(&info(true, AutonomyLevel::Observe)),
            Reversibility::Observe
        );
        assert_eq!(
            reversibility_band(&info(true, AutonomyLevel::ActWithinPolicy)),
            Reversibility::Reversible
        );
        assert_eq!(
            reversibility_band(&info(true, AutonomyLevel::ConfirmEachAction)),
            Reversibility::OutwardReversible
        );
    }

    #[test]
    fn the_classifier_blocks_the_irreversible_rather_than_confirming_it() {
        use crate::application::AutonomyEnvelope;
        let irreversible = CapabilityInfo {
            id: "book",
            name: "Book",
            description: "",
            category: "",
            reaches_external: true,
            reversible: false,
            autonomy: AutonomyLevel::ConfirmEachAction,
            configured: true,
            needs: "",
            settings: &[],
        };
        // Blocked outright, not merely confirmed — even fully widened (ADR 0024).
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };
        assert_eq!(classify(&irreversible, &widened), Decision::Block);
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
                    reversible: false,
                    autonomy: AutonomyLevel::ConfirmEachAction,
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
    fn geocode_candidates_simplify_city_state() {
        assert_eq!(
            geocode_candidates("Charlotte NC"),
            vec!["Charlotte NC", "Charlotte"]
        );
        assert_eq!(
            geocode_candidates("Charlotte, NC"),
            vec!["Charlotte, NC", "Charlotte"]
        );
        assert_eq!(geocode_candidates("Boston"), vec!["Boston"]);
        assert_eq!(geocode_candidates("San Francisco"), vec!["San Francisco"]);
    }

    #[test]
    fn zip_detector_ignores_non_zip_input_without_a_network_call() {
        // Non-5-digit or non-numeric input must fall through (Ok(None)), so a place
        // name never triggers the ZIP lookup.
        assert!(resolve_us_zip("Charlotte").unwrap().is_none());
        assert!(resolve_us_zip("2827").unwrap().is_none());
        assert!(resolve_us_zip("abcde").unwrap().is_none());
        assert!(resolve_us_zip("Boston, MA").unwrap().is_none());
    }

    #[test]
    fn outbound_tripwire_flags_secrets_but_not_ordinary_text() {
        // Ordinary queries and URLs are NOT flagged (no false positives).
        assert_eq!(
            scan_outbound_secret("what's the weather in Charlotte"),
            None
        );
        assert_eq!(
            scan_outbound_secret("https://example.com/articles/2026/summer-festival"),
            None
        );
        assert_eq!(scan_outbound_secret("{\"location\":\"28277\"}"), None);
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
            redact_emails_in_text("weather in Charlotte tomorrow"),
            "weather in Charlotte tomorrow"
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
            <item><title>Storms hit Charlotte &amp; the region</title><link>https://ex.com/a</link>\
              <source url=\"https://www.wcnc.com\">WCNC</source></item>\
            <item><title><![CDATA[City council votes tonight]]></title></item>\
            <item><title>Third &#39;big&#39; story</title></item>\
            </channel></rss>";
        let items = extract_rss_items(xml, 6);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].0, "Storms hit Charlotte & the region");
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
            "query": "Charlotte news",
            "count": 2,
            "headlines": ["Council meets tonight", "Road closures downtown"],
            "note": "",
        });
        let text = LocalNewsCapability.summarize(&out);
        assert!(text.contains("Charlotte news"));
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
}
