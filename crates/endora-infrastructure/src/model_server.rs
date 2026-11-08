//! Talking to a model server: whether an address is on the house's network, which models an
//! OpenAI-compatible endpoint offers, whether an endpoint and key work, and what a local Ollama
//! has loaded. Used by the AI screen, the team's relay (ADR 0102) and the Mac's brain (ADR 0130).

use serde_json::{Value, json};

/// What a local Ollama says it has loaded (`GET {origin}/api/ps`), or `None` if it does not
/// answer within a few seconds or is not Ollama. Read-only; for the AI screen.
#[must_use]
pub fn ollama_loaded(origin: &str) -> Option<serde_json::Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(3)))
        .build()
        .into();
    let mut resp = agent.get(&format!("{origin}/api/ps")).call().ok()?;
    resp.body_mut().read_json::<serde_json::Value>().ok()
}

/// Whether a URL points at the person's own network — loopback, link-local, RFC 1918 and
/// IPv6 unique-local addresses, or a `.local`/`.lan`/bare hostname. An entry here is reached
/// directly with the raw conversation; any other goes through the door (ADR 0069, 0105), so
/// a mistake here is a cloud model reached with no disguise. The host is what follows any
/// user-info (`user:key@`), and an address in brackets is judged as an address.
#[must_use]
pub fn is_private_endpoint(url: &str) -> bool {
    let url = url.trim();
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(bracketed) = host_and_port.strip_prefix('[') {
        let Some((address, _)) = bracketed.split_once(']') else {
            return false;
        };
        address
    } else if host_and_port.matches(':').count() > 1 {
        // An IPv6 address without its brackets: the whole of it, never its first group.
        host_and_port
    } else {
        host_and_port.split(':').next().unwrap_or_default()
    };
    let host = host.to_lowercase();
    if host.is_empty() {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let v4_local =
            |v4: std::net::Ipv4Addr| v4.is_loopback() || v4.is_private() || v4.is_link_local();
        return match ip {
            std::net::IpAddr::V4(v4) => v4_local(v4),
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return v4_local(v4);
                }
                let first = v6.segments()[0];
                v6.is_loopback()
                    // fe80::/10, link-local
                    || first & 0xffc0 == 0xfe80
                    // fc00::/7, unique local
                    || first & 0xfe00 == 0xfc00
            }
        };
    }
    if host.contains(':') {
        return false;
    }
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".lan") {
        return true;
    }
    // A bare single-label hostname ("nas", "host.docker.internal" is multi-label but
    // known-local) resolves on the person's own network or not at all.
    host == "host.docker.internal" || !host.contains('.')
}

/// Whether `url` reaches only the house's network: private by its text
/// ([`is_private_endpoint`]), or a name that resolves, now, to private addresses only, such as
/// a laptop's name as the house's router hands it out, which follows the laptop when its
/// address changes (ADR 0130). A name that resolves to nothing is not known to be at home.
#[must_use]
pub fn reaches_only_the_house(url: &str) -> bool {
    if is_private_endpoint(url) {
        return true;
    }
    let rest = url
        .trim()
        .split_once("://")
        .map_or(url.trim(), |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if authority.is_empty() || authority.starts_with('[') {
        return false;
    }
    let target = if authority.contains(':') {
        authority.to_owned()
    } else if url.trim().starts_with("https://") {
        format!("{authority}:443")
    } else {
        format!("{authority}:80")
    };
    let Ok(found) = std::net::ToSocketAddrs::to_socket_addrs(&target) else {
        return false;
    };
    all_private(&found.collect::<Vec<_>>())
}

/// Whether every address is on the house's network, and there is at least one.
fn all_private(addrs: &[std::net::SocketAddr]) -> bool {
    !addrs.is_empty()
        && addrs
            .iter()
            .all(|a| is_private_endpoint(&format!("http://{a}/")))
}

/// Lists the model ids an OpenAI-compatible endpoint offers (`GET {base}/models`)
/// — so the console can let the person pick a model after entering the endpoint +
/// key, instead of typing it. `api_key` is sent as a bearer when non-empty
/// (needed for cloud providers; keyless for local runtimes like Ollama).
///
/// # Errors
/// A message if the endpoint is unreachable or returns an error/unexpected shape.
pub fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let mut req = agent.get(&url);
    if !api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {api_key}"));
    }
    let mut response = req.call().map_err(|e| e.to_string())?;
    if response.status().as_u16() >= 300 {
        return Err(format!("endpoint returned status {}", response.status()));
    }
    let json: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = json["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Verifies an OpenAI-compatible endpoint **and API key actually work** — the
/// settings "Test connection" check. When a `model` is given it sends a *minimal*
/// chat completion, which is the real operation the key must authorize (many
/// endpoints serve `/models` without auth but reject completions, so listing models
/// alone can pass with a bad key); with no model it falls back to listing `/models`
/// as a reachability check. Returns a short human-readable success detail. The
/// `api_key`, if non-empty, is sent as a bearer token.
///
/// # Errors
/// A friendly message when the endpoint is unreachable, the key is rejected, the
/// model is unknown, or the reply is unexpected.
pub fn test_connection(base_url: &str, api_key: &str, model: &str) -> Result<String, String> {
    if base_url.trim().is_empty() {
        return Err("enter the endpoint first".to_owned());
    }
    if model.trim().is_empty() {
        // No model chosen yet: just confirm the endpoint is reachable + lists models.
        let models = list_models(base_url, api_key)?;
        let n = models.len();
        return Ok(format!(
            "Reached the endpoint — {n} model{} available. Pick a model and test again to \
             confirm it answers with your key.",
            if n == 1 { "" } else { "s" }
        ));
    }
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    // Read the HTTP status ourselves (don't turn 4xx into a transport error), so we
    // can tell "key rejected" from "model not found" from "unreachable".
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut req = agent.post(&url).header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {api_key}"));
    }
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
        "stream": false,
    });
    let mut response = req.send_json(&body).map_err(|e| {
        format!("couldn't reach the endpoint — check the URL and your network ({e})")
    })?;
    let status = response.status().as_u16();
    if status >= 300 {
        return Err(match status {
            401 | 403 => format!("the API key was rejected ({status}) — check the key"),
            404 => format!("the model '{model}' wasn't found at this endpoint (404)"),
            429 => "rate-limited (429) — the key works, but the provider is throttling".to_owned(),
            s => format!("the endpoint returned status {s}"),
        });
    }
    // A 2xx with a completion means the endpoint + key + model all work.
    let json: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("the endpoint replied but not in the expected shape ({e})"))?;
    if json["choices"].as_array().is_some_and(|c| !c.is_empty()) {
        Ok(format!("Connected — '{model}' answered with your key."))
    } else {
        Err("the endpoint replied without a completion — is the model id right?".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::test_connection;

    #[test]
    fn a_house_address_is_told_from_a_cloud_one() {
        // Base turns carry the raw conversation; only the deep rung has the pseudonym
        // door (ADR 0069). A public address in the base list would move everything off
        // the device on whichever days it answered.
        for private in [
            "http://192.168.1.10:11434/v1",
            "http://10.0.0.5:8080/v1",
            "http://localhost:11434/v1",
            "http://nas:11434/v1",
            "http://host.docker.internal:11434/v1",
            "http://mymac.local:11434/v1",
            "http://172.16.0.5:8080/v1",
            "http://[::1]:11434/v1",
            "http://[fe80::1]:11434/v1",
            "http://[fd12:3456::1]:8080/v1",
            "http://[::ffff:192.168.1.10]:11434/v1",
            "http://user:secret@nas:11434/v1",
        ] {
            assert!(super::is_private_endpoint(private), "{private}");
        }
        for public in [
            "https://api.openai.com/v1",
            "https://api.deepseek.com/v1",
            "http://8.8.8.8:11434/v1",
            "",
            // Who is asking is not where it goes: the host follows the '@'.
            "https://user:token@example.com/v1",
            "https://user@203.0.113.7/v1",
            // An address written as IPv6 is judged as an address, not as a bare name.
            "https://[2606:4700::1111]/v1",
            "https://[2001:db8::1]:8443/v1",
            "https://[::ffff:8.8.8.8]/v1",
        ] {
            assert!(!super::is_private_endpoint(public), "{public}");
        }
    }

    #[test]
    fn a_name_reaches_only_the_house_when_everything_it_resolves_to_is_private() {
        let at = |s: &str| s.parse::<std::net::SocketAddr>().unwrap();
        assert!(super::all_private(&[at("192.168.1.10:8080")]));
        assert!(super::all_private(&[
            at("192.168.1.10:8080"),
            at("[fd00::10]:8080")
        ]));
        assert!(!super::all_private(&[
            at("192.168.1.10:8080"),
            at("8.8.8.8:8080")
        ]));
        assert!(!super::all_private(&[]));
        // A private address by its text needs no lookup; a public one never passes.
        assert!(super::reaches_only_the_house("http://192.168.1.10:8080/v1"));
        assert!(!super::reaches_only_the_house("http://8.8.8.8:8080/v1"));
        // A name the text check does not know, that resolves to a private address, reaches only
        // the house: "localhost." stands in for a laptop's router-given name.
        assert!(!super::is_private_endpoint("http://localhost.:8080/v1"));
        assert!(super::reaches_only_the_house("http://localhost.:8080/v1"));
        // A name that resolves to nothing is not known to be at home.
        assert!(!super::reaches_only_the_house(
            "http://no-such-mac.invalid:8080/v1"
        ));
    }

    #[test]
    fn test_connection_rejects_a_blank_endpoint_before_any_call() {
        // The guard runs before any network I/O, so this is deterministic offline.
        let err = test_connection("   ", "some-key", "gpt-4o").unwrap_err();
        assert!(err.contains("endpoint"), "unexpected: {err}");
    }
}
