//! Credential shapes (ADR 0051): the data-loss tripwire's knowledge of what a key, a token or a
//! private key looks like, shared by the contexts that keep one out of what they pass on: the
//! capabilities' outbound calls, and the team's lessons (ADR 0142, amended 2026-10-31).

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

/// Classifies a single token as a known credential shape, or `None`.
pub fn classify_secret_token(t: &str) -> Option<&'static str> {
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
