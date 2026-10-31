//! Credential shapes (ADR 0051): the data-loss tripwire's knowledge of what a key, a token or a
//! private key looks like, shared by the contexts that keep one out of what they pass on: the
//! capabilities' outbound calls and logs, and the team's lessons (ADR 0142, amended 2026-10-31).
//! One scanner: [`scan_outbound_secret`] finds a credential's shape, [`scrub_secrets`] replaces
//! it and a value given to a secret's name, and [`secret_in`] says whether either is there.

/// What secret `text` holds, if any: a credential's shape ([`scan_outbound_secret`]), or what
/// [`scrub_secrets`] would hide (`DB_PASSWORD=…`, `Bearer …`, a link's password).
#[must_use]
pub fn secret_in(text: &str) -> Option<&'static str> {
    scan_outbound_secret(text).or_else(|| (scrub_secrets(text) != text).then_some("a secret"))
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
    // Split on what ends a word in a log line AND the URL delimiters, so a secret embedded in a
    // query string (…?token=sk-…) is isolated as its own token.
    text.split(|c: char| ends_a_word(c) || matches!(c, '/' | '#' | '@' | '|'))
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

/// A log or other text off a machine, as a team member's model may read it (ADR 0106): every
/// secret this file knows the shape of is replaced with `[redacted]`, and the rest of each line
/// is kept, so it still says what went wrong. Besides [`scan_outbound_secret`]'s shapes: the
/// value given to a name that says it is secret (`DB_PASSWORD=…`, `"api_key": "…"`,
/// `X-Api-Key: …`), the word after `Bearer` or `Basic`, and a private key's whole block. A
/// block cut short hides everything after its start.
#[must_use]
pub fn scrub_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_a_key = false;
    for line in text.split_inclusive('\n') {
        let begins = line.contains("-----BEGIN") && line.contains("PRIVATE KEY");
        let ends = line.contains("-----END") && line.contains("PRIVATE KEY");
        if !in_a_key && !begins {
            out.push_str(&scrub_line(&hide_link_credentials(line)));
            continue;
        }
        if !in_a_key {
            out.push_str("[redacted: a private key]\n");
        }
        in_a_key = !ends;
    }
    out
}

/// Where a word in a log line ends: the delimiters [`scan_outbound_secret`] splits on, but not
/// `/`, `@`, `#` or `|`, which a password may hold. A word is split on those too before its
/// pieces are classified.
fn ends_a_word(c: char) -> bool {
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
                | ':'
        )
}

/// Whether a name (lower-cased) says its value is a secret: it ends in a long word that only a
/// secret goes by (`db_password`, `client_secret`), or is, or ends after a `_`, `-` or `.` in, a
/// short one (`key`, `x-api-key`, `auth`), so `monkey` and `bypass` are left alone.
fn names_a_secret(name: &str) -> bool {
    const LONG: [&str; 7] = [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "credential",
        "credentials",
    ];
    const SHORT: [&str; 6] = ["pass", "pwd", "key", "auth", "authorization", "cookie"];
    LONG.iter().any(|w| name.ends_with(w))
        || SHORT.iter().any(|w| {
            name == *w
                || ["_", "-", "."]
                    .iter()
                    .any(|sep| name.ends_with(&format!("{sep}{w}")))
        })
}

/// A line with the credentials of each link in it (`https://user:password@host`) replaced: what
/// comes between `://` and the last `@` before the link's host ends. The scheme and the host
/// stay, so the line still says where it went.
fn hide_link_credentials(line: &str) -> std::borrow::Cow<'_, str> {
    if !line.contains("://") || !line.contains('@') {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + 3);
        out.push_str(before);
        let host_ends = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'' | '<' | '>')
            })
            .unwrap_or(after.len());
        let authority = &after[..host_ends];
        match authority.rfind('@') {
            Some(user_ends) => {
                out.push_str("[redacted]");
                out.push_str(&authority[user_ends..]);
            }
            None => out.push_str(authority),
        }
        rest = &after[host_ends..];
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// One line of [`scrub_secrets`].
fn scrub_line(line: &str) -> String {
    let runs = runs_of(line);
    let mut out = String::with_capacity(line.len());
    // Hiding: the next word (`Some(None)`), or everything up to a closing quote.
    let mut hide: Option<Option<char>> = None;
    let mut skip = false;
    for (i, &(is_word, run)) in runs.iter().enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        if let Some(Some(quote)) = hide {
            if let Some(at) = run.find(quote).filter(|_| !is_word) {
                out.push_str(&run[at..]);
                hide = None;
            }
            continue;
        }
        if !is_word {
            out.push_str(run);
            continue;
        }
        let lower = run.to_ascii_lowercase();
        // A scheme before a credential stays, and the credential after it goes.
        if lower == "bearer" || lower == "basic" || (lower == "token" && hide.is_some()) {
            out.push_str(run);
            hide = Some(None);
            continue;
        }
        if hide.take().is_some() {
            out.push_str("[redacted]");
            continue;
        }
        let given = runs.get(i + 1).map_or("", |(_, r)| *r);
        // `name=value`, `name: value`, `"name": "value"`; not `name=""`, which gives nothing.
        let gives = given.contains(['=', ':'])
            && given
                .chars()
                .all(|c| matches!(c, '=' | ':' | ' ' | '"' | '\''))
            && !given.contains("\"\"")
            && !given.contains("''");
        if !gives || !names_a_secret(&lower) {
            out.push_str(&scrub_pieces(run));
            continue;
        }
        out.push_str(run);
        // A quoted value is hidden to its closing quote, whatever it holds.
        match given.chars().last().filter(|c| matches!(c, '"' | '\'')) {
            Some(quote) => {
                out.push_str(given);
                out.push_str("[redacted]");
                hide = Some(Some(quote));
                skip = true;
            }
            None => hide = Some(None),
        }
    }
    // A quote never closed hid the rest of the line, but not its end.
    if matches!(hide, Some(Some(_))) && line.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// A line as runs of word and of delimiters, in order.
fn runs_of(line: &str) -> Vec<(bool, &str)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut word = None;
    for (i, c) in line.char_indices() {
        let is_word = !ends_a_word(c);
        if word != Some(is_word) {
            if let Some(was) = word {
                runs.push((was, &line[start..i]));
            }
            start = i;
            word = Some(is_word);
        }
    }
    if let Some(was) = word {
        runs.push((was, &line[start..]));
    }
    runs
}

/// A word with any piece of it (split on `/`, `@`, `#` and `|`) that has a credential's shape
/// replaced.
fn scrub_pieces(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut piece = String::new();
    // A key that ends a sentence is still a key: its full stop is kept, after the mark.
    let flush = |piece: &mut String, out: &mut String| {
        let key = piece.trim_end_matches(['.', '!', '`']);
        if classify_secret_token(key).is_some() {
            out.push_str("[redacted]");
            out.push_str(&piece[key.len()..]);
        } else {
            out.push_str(piece);
        }
        piece.clear();
    };
    for c in word.chars() {
        if matches!(c, '/' | '@' | '#' | '|') {
            flush(&mut piece, &mut out);
            out.push(c);
            continue;
        }
        piece.push(c);
    }
    flush(&mut piece, &mut out);
    out
}
