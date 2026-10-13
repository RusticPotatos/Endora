use super::*;

pub(super) fn normalize_words(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- helpers ---------------------------------------------------------------

/// Reads a service address the person typed, and makes it a URL.
///
/// `192.168.1.10:8123` is what someone types when asked for their Home Assistant address,
/// and it is not a URL — every request built from it fails with `http: invalid format`.
///
/// This was live and invisible. Endora's **direct reach** into the house had been dead:
/// presence, live states, the standing-trouble watch and the facts behind an answer all
/// silently returned nothing, while the MCP tools kept working because that server talks
/// to Home Assistant on its own. Nothing looked broken, because a channel that cannot
/// answer is indistinguishable from a service with nothing to say — the same trap
/// [0054](../../docs/adr/0054-other-peoples-services.md) records about defaulted port
/// methods, arriving this time through a text box.
///
/// **`http` rather than `https`** when the scheme is missing: these are addresses on the
/// person's own network, typed as a bare host and port, and that is what such a service
/// is almost always serving. Someone who needs TLS has a hostname and will type the
/// scheme.
pub(crate) fn as_url(typed: &str) -> String {
    let address = typed.trim().trim_end_matches('/');
    if address.is_empty() || address.contains("://") {
        return address.to_owned();
    }
    format!("http://{address}")
}

/// A **direct** HTTP agent — no egress proxy. Used for trusted internal calls (the
/// local vision model), which must never be routed through a VPN/proxy.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// The configured egress proxy, if any. `ENDORA_EGRESS_PROXY` is an `http://`,
/// `https://`, or `socks5://` URL (trusted deployment config). See
/// [`external_agent`].
pub(super) fn egress_proxy() -> Option<ureq::Proxy> {
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
/// container (ADR 0051). Loosely coupled: if the proxy is down, only external skills
/// fail; the app keeps running.
pub(super) fn external_agent() -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15)));
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    builder.build().into()
}

// ---- Answers worth keeping (ADR 0061) --------------------------------------

/// How long an answer is kept when the source says nothing, and the shortest it is ever
/// kept even when the source says less.
///
/// A source is not owed unlimited trust about its own freshness. Something claiming
/// `max-age=0` would make this pointless; the floor decides instead.
pub(super) const KEEP_AT_LEAST_MS: u64 = 60_000;

/// The longest an answer is kept, whatever the source claims.
///
/// A ceiling because a source claiming a year would otherwise make the butler wrong for a
/// year, and being confidently out of date is worse than being slow.
pub(super) const KEEP_AT_MOST_MS: u64 = 6 * 60 * 60 * 1_000;

/// How many answers are remembered at once. Oldest goes first.
pub(super) const MOST_ANSWERS_KEPT: usize = 256;

/// One remembered answer.
pub(super) struct Remembered {
    body: String,
    /// When it stops being served without asking again.
    fresh_until_ms: u64,
    /// When it arrived — used to decide what to forget first, and nothing else.
    stored_ms: u64,
}

/// Everything remembered, in memory only (ADR 0061).
///
/// Never written to disk. A cache keyed by what somebody asked is a **record of questions**,
/// and persisting it would create an obligation to purge it on *forget everything* that a
/// warm start does not justify.
pub(super) static REMEMBERED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(u64, u64), Remembered>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A transport failure, said the way a person would say it.
///
/// Shown live, as the whole of an answer:
///
/// ```text
/// error: unavailable: io: invalid peer certificate: certificate expired: verification
/// time 1785717318 (UNIX), but certificate is not valid after 1543938436 (241778882
/// seconds ago)
/// ```
///
/// Every word of that is true and none of it is an answer. The person asked a question; a
/// site they have never heard of has a certificate that lapsed in 2018, and there is nothing
/// they can do about it. **What matters is that the page could not be read and why in one
/// clause** — the rest is plumbing, and the butler does not narrate plumbing.
///
/// Mapped on what the message contains rather than on a status code, because these arrive
/// as prose from a transport that owes us no shape. Anything unrecognised is passed through:
/// a wrong guess would be worse than a technical sentence, and unrecognised is where the
/// next unknown failure lives.
#[must_use]
pub fn plainly(said: &str) -> String {
    let lower = said.to_ascii_lowercase();
    let plain = if lower.contains("certificate expired") || lower.contains("certificatexpired") {
        "that site's security certificate has expired, so I would not trust the page"
    } else if lower.contains("certificate") {
        "that site's security certificate did not check out, so I would not trust the page"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "that site took too long to answer"
    } else if lower.contains("dns") || lower.contains("resolve") {
        "I could not find that site at all"
    } else if lower.contains("connection refused") || lower.contains("connect") {
        "that site would not let me connect"
    } else {
        return said.to_owned();
    };
    plain.to_owned()
}

/// A fingerprint of a request — enough to recognise it again, not enough to rebuild it.
///
/// **The URL is never stored.** It carries the API key of whatever is being asked, and often
/// the person's own town. The node holds those elsewhere and must; what this refuses to do
/// is create a *second* place they can be read from. There is nothing here to redact in a
/// log, because there is nothing here to print.
///
/// Two independent hashes rather than one, because a collision would not be a slow answer —
/// it would be another question's answer, delivered confidently.
pub(super) fn fingerprint(url: &str) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let once = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut h);
        h.finish()
    };
    let twice = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // A different starting state, so the two are not the same function twice.
        "endora/0061".hash(&mut h);
        url.hash(&mut h);
        h.finish()
    };
    (once, twice)
}

/// How long the source says its answer is good for, within bounds.
///
/// `Cache-Control: max-age` decides it, per response, per source — which is the whole reason
/// there is no table of which source is hourly and which is monthly. Such a table would go
/// stale, and it is exactly the per-integration knowledge that belongs nowhere near shared
/// code.
#[must_use]
pub fn keep_for_ms(cache_control: Option<&str>) -> u64 {
    let Some(said) = cache_control else {
        return KEEP_AT_LEAST_MS;
    };
    let said = said.to_ascii_lowercase();
    // The one directive worth obeying exactly: the source is saying do not keep this at all.
    // `max-age=0` is NOT that — it is a claim about freshness, and a claim about freshness is
    // the thing the floor exists to bound.
    if said.contains("no-store") {
        return 0;
    }
    let value_of = |name: &str| {
        said.split(',').find_map(|part| {
            part.trim()
                .strip_prefix(name)?
                .trim()
                .parse::<u64>()
                .ok()
                .map(|seconds| seconds.saturating_mul(1000))
        })
    };
    // `s-maxage` first wherever both appear: it is the one addressed to a shared cache,
    // which is what this is. Order in the header must not decide it.
    value_of("s-maxage=")
        .or_else(|| value_of("max-age="))
        .map_or(KEEP_AT_LEAST_MS, |ms| {
            ms.clamp(KEEP_AT_LEAST_MS, KEEP_AT_MOST_MS)
        })
}

/// Forgets the oldest answers once there are too many.
pub(super) fn forget_the_oldest(kept: &mut std::collections::HashMap<(u64, u64), Remembered>) {
    while kept.len() > MOST_ANSWERS_KEPT {
        let Some(oldest) = kept
            .iter()
            .min_by_key(|(_, r)| r.stored_ms)
            .map(|(k, _)| *k)
        else {
            return;
        };
        kept.remove(&oldest);
    }
}

/// GETs a URL, remembering the answer for as long as the source says it is good (ADR 0061).
///
/// One place, so a skill written the ordinary way is cached without opting in and a new one
/// does nothing to take part. The capability runner was the tempting layer and the wrong
/// one: by the time a result reaches it the origin's headers are gone, and recovering them
/// would mean every skill reporting its own freshness.
///
/// **A stale answer beats no answer.** If the source is down or the quota is spent and there
/// is an old answer, it is served — the failure that has cost most here is a screen quietly
/// saying it knows nothing when it does.
pub(super) fn get_remembering(
    url: &str,
    ua: Option<&str>,
    max_bytes: usize,
) -> Result<String, CapabilityError> {
    use std::io::Read;
    let id = fingerprint(url);
    let now = now_ms();
    if let Ok(kept) = REMEMBERED.lock() {
        if let Some(found) = kept.get(&id) {
            if now < found.fresh_until_ms {
                return Ok(found.body.clone());
            }
        }
    }

    let mut request = external_agent().get(url);
    if let Some(ua) = ua {
        request = request.header("User-Agent", ua);
    }
    let asked = request.call().and_then(|mut resp| {
        let keep = keep_for_ms(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
        );
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(max_bytes as u64)
            .read_to_end(&mut buf)?;
        Ok((String::from_utf8_lossy(&buf).into_owned(), keep))
    });

    match asked {
        Ok((body, keep)) => {
            if keep > 0 {
                if let Ok(mut kept) = REMEMBERED.lock() {
                    kept.insert(
                        id,
                        Remembered {
                            body: body.clone(),
                            fresh_until_ms: now.saturating_add(keep),
                            stored_ms: now,
                        },
                    );
                    forget_the_oldest(&mut kept);
                }
            }
            Ok(body)
        }
        Err(e) => {
            // Stale rather than nothing.
            if let Ok(kept) = REMEMBERED.lock() {
                if let Some(found) = kept.get(&id) {
                    return Ok(found.body.clone());
                }
            }
            Err(CapabilityError::Unavailable(plainly(&e.to_string())))
        }
    }
}

/// GETs a URL and returns the body as text (size-capped), for the info skills.
pub(super) fn http_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    get_remembering(url, None, max_bytes)
}

pub(super) fn str_field<'a>(input: &'a Value, key: &str) -> Result<&'a str, CapabilityError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CapabilityError::BadInput(format!("missing '{key}'")))
}

/// GET with an explicit `User-Agent` (some APIs, e.g. the US NWS, require one).
pub(super) fn http_get_text_ua(
    url: &str,
    ua: &str,
    max_bytes: usize,
) -> Result<String, CapabilityError> {
    get_remembering(url, Some(ua), max_bytes)
}

// ---- Egress guard (SSRF protection for model/person-provided URLs) ----------

/// Rejects a URL whose host is, or resolves to, a non-public address — closing the
/// SSRF hole where a model-provided URL could reach the internal network or a cloud
/// metadata endpoint (ADR 0051). Only for **arbitrary** URLs; the trusted internal
/// model calls and constant-host API skills do not use this.
pub(super) fn guard_egress(url: &str) -> Result<(), CapabilityError> {
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
pub(super) fn host_and_port(url: &str) -> Option<(String, u16)> {
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

/// The data-loss tripwire (ADR 0051): scans text about to leave the machine (an
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

/// Query minimization (ADR 0051): redacts personal identifiers from an external
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

/// [`redact_pii_in_value`] for plain text — the shape a typed question arrives in.
#[must_use]
pub fn redact_pii_in_text(text: &str) -> String {
    redact_emails_in_text(text)
}

/// Replaces whole-word email addresses in free text with `[redacted-email]`. Splits
/// on spaces so a bare URL (one word, not an email) is never altered — only a
/// standalone address in a query is caught.
pub(super) fn redact_emails_in_text(text: &str) -> String {
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
pub(super) fn looks_like_email(s: &str) -> bool {
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
pub(super) fn classify_secret_token(t: &str) -> Option<&'static str> {
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
/// manually, re-guarding each hop (ADR 0051). For model/person-provided URLs.
pub(super) fn guarded_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    let bytes = guarded_get_bytes(url, max_bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Guarded byte fetch with manual, re-guarded redirect following (bounded).
pub(super) fn guarded_get_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, CapabilityError> {
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
pub(super) fn resolve_redirect(base: &str, location: &str) -> String {
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
pub(super) fn http_post_json(
    url: &str,
    body: &Value,
    max_bytes: usize,
) -> Result<String, CapabilityError> {
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
pub(super) fn base64_encode(bytes: &[u8]) -> String {
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
pub(super) fn resolve_point(input: &Value) -> Result<(f64, f64, String), CapabilityError> {
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
    // "New York NY" or "New York, NY". So try the full query, then simpler forms
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
/// "New York NY" / "New York, NY" both fall back to "New York").
pub(super) fn geocode_candidates(q: &str) -> Vec<String> {
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
pub(super) fn resolve_us_zip(q: &str) -> Result<Option<(f64, f64, String)>, CapabilityError> {
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
