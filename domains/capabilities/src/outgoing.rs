//! What may leave the house in a branch (ADR 0115, amended 2026-10-31). Before the node pushes an
//! accepted branch to the owner's public fork without a card, its whole diff and its commit
//! messages are scanned here, by code and never by a model:
//!
//! - by **shape**, the rules of `scripts/check-nothing-personal.mjs` (a real mailbox, a real
//!   network address, coordinates, a phone number, a long hex string), ported line for line;
//! - by the kernel's **secret tripwire** (`endora_kernel::secrets`: a private key, a provider's
//!   key or token);
//! - and against the owner's **own values**, which the node reads from `ENDORA_PERSONAL_VALUES`
//!   in its environment, passed by the deploy from the git-ignored `local.mk` (or the role's
//!   `endora_personal_values` from the owner's vault), never from a tracked file.
//!
//! A finding says what and where, never the value. Only what the branch adds is read: a line
//! the diff removes is leaving the fork, not entering it.
//!
//! Also here, by the same reading of the diff: what a change **needs of the owner** before it
//! is deployed, a role default that now reads a `vault_*` variable it did not before, or an
//! assert that a variable is set.

use endora_kernel::secrets::scan_outbound_secret;

/// One thing found, and where: never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What kind of thing ("a real network address"), or the variable a change needs.
    pub what: String,
    /// Where: `path line N`, `commit <sha> message`, or a file.
    pub at: String,
}

/// Mailboxes that are nobody's, as the repository check has them.
fn mailbox_is_fictional(mailbox: &str) -> bool {
    let lower = mailbox.to_ascii_lowercase();
    let (local, domain) = lower.split_once('@').unwrap_or(("", &lower));
    [
        "example.com",
        "example.org",
        "example.net",
        "b.com",
        "b.co",
        "x.org",
    ]
    .iter()
    .any(|d| domain == *d || domain.ends_with(&format!(".{d}")) || domain.ends_with(d))
        || domain.ends_with(".invalid")
        || domain == "invalid"
        || local == "noreply"
        || domain.ends_with(".noreply.github.com")
        || lower == "git@github.com"
        || [
            "service", "socket", "timer", "target", "mount", "path", "slice", "scope", "device",
        ]
        .iter()
        .any(|unit| domain.ends_with(&format!(".{unit}")))
}

/// Addresses that teach something and belong to no one, as the repository check has them.
const ADDRESS_IS_AN_EXAMPLE: [&str; 9] = [
    "0.0.0.0",
    "1.1.1.1",
    "8.8.8.8",
    "10.0.0.1",
    "10.0.0.5",
    "100.64.0.0",
    "127.0.0.1",
    "169.254.169.254",
    "192.168.1.10",
];

/// An address that cannot be somebody's host: a network, Docker's range, loopback, link-local,
/// multicast, or the documentation ranges.
fn is_not_a_host(o: [u32; 4]) -> bool {
    let [a, b, c, d] = o;
    d == 0
        || (a == 172 && (16..=31).contains(&b))
        || a == 127
        || (a == 169 && b == 254)
        || (224..=239).contains(&a)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Real-looking mailboxes in a line.
fn mailboxes(line: &str) -> Vec<String> {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let host = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    for (at, _) in chars.iter().enumerate().filter(|(_, c)| **c == '@') {
        let mut start = at;
        while start > 0 && local(chars[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < chars.len() && host(chars[end]) {
            end += 1;
        }
        if start == at || end == at + 1 {
            continue;
        }
        let domain: String = chars[at + 1..end].iter().collect();
        let domain = domain.trim_end_matches(['.', '-']);
        let Some((_, tld)) = domain.rsplit_once('.') else {
            continue;
        };
        if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let local_part: String = chars[start..at].iter().collect();
        found.push(format!("{local_part}@{domain}"));
    }
    found
}

/// Real-looking host addresses in a line.
fn addresses(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && (word(chars[i - 1]) || chars[i - 1] == '.')) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
            end += 1;
        }
        let run: String = chars[i..end].iter().collect();
        let after_word = end < chars.len() && word(chars[end]);
        i = end;
        if after_word {
            continue;
        }
        let parts: Vec<&str> = run.trim_end_matches('.').split('.').collect();
        if parts.len() < 4 {
            continue;
        }
        let octets: Vec<Option<u32>> = parts[..4]
            .iter()
            .map(|p| {
                (!p.is_empty() && p.len() <= 3)
                    .then(|| p.parse().ok())
                    .flatten()
                    .filter(|n| *n <= 255)
            })
            .collect();
        let [Some(a), Some(b), Some(c), Some(d)] = octets[..] else {
            continue;
        };
        let address = format!("{a}.{b}.{c}.{d}");
        if ADDRESS_IS_AN_EXAMPLE.contains(&address.as_str()) || is_not_a_host([a, b, c, d]) {
            continue;
        }
        found.push(address);
    }
    found
}

/// Whether a line sets a coordinate: a `lat`, `lon`, `lng`, `latitude` or `longitude` key, `:` or
/// `=`, and a number with three decimals or more.
fn coordinates(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    for key in ["latitude", "longitude", "lat", "lon", "lng"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(key).map(|n| n + from) {
            from = at + key.len();
            if at > 0 && word(lower[..at].chars().next_back().unwrap_or(' ')) {
                continue;
            }
            let rest = lower[at + key.len()..].trim_start_matches(['"', '\'']);
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix([':', '=']) else {
                continue;
            };
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('-').unwrap_or(rest);
            let whole = rest.chars().take_while(char::is_ascii_digit).count();
            if !(1..=3).contains(&whole) {
                continue;
            }
            let after = &rest[whole..];
            let Some(fraction) = after.strip_prefix('.') else {
                continue;
            };
            if fraction.chars().take_while(char::is_ascii_digit).count() >= 3 {
                return true;
            }
        }
    }
    false
}

/// Whether a line holds a North American phone number: an area code (in brackets or not), then
/// three and four digits, each after a space, a dot or a dash.
fn phone(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let digits = |from: usize, n: usize| -> bool {
        from + n <= chars.len() && chars[from..from + n].iter().all(char::is_ascii_digit)
    };
    let sep = |at: usize| at < chars.len() && " .-".contains(chars[at]);
    for start in 0..chars.len() {
        if start > 0 && word(chars[start - 1]) {
            continue;
        }
        let mut i = start;
        let open = chars[i] == '(';
        if open {
            i += 1;
        }
        if !digits(i, 3) || !('2'..='9').contains(&chars[i]) {
            continue;
        }
        i += 3;
        if open {
            if chars.get(i) != Some(&')') {
                continue;
            }
            i += 1;
        }
        if !sep(i) || !digits(i + 1, 3) || !sep(i + 4) || !digits(i + 5, 4) {
            continue;
        }
        let end = i + 9;
        if end < chars.len() && word(chars[end]) {
            continue;
        }
        return true;
    }
    false
}

/// Whether a line holds a long run of lowercase hex (32 or more), not a `sha256:` digest.
fn long_hex(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
    let mut i = 0;
    while i < chars.len() {
        if !hex(chars[i]) || (i > 0 && word(chars[i - 1])) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && hex(chars[end]) {
            end += 1;
        }
        let digest = chars[..i].iter().collect::<String>().ends_with("sha256:");
        if end - i >= 32 && !(end < chars.len() && word(chars[end])) && !digest {
            return true;
        }
        i = end.max(i + 1);
    }
    false
}

/// What one line holds that must not leave the house, by kind.
fn kinds_in(line: &str, own: &[String]) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if mailboxes(line).iter().any(|m| !mailbox_is_fictional(m)) {
        kinds.push("a real mailbox");
    }
    if !addresses(line).is_empty() {
        kinds.push("a real network address");
    }
    if coordinates(line) {
        kinds.push("coordinates");
    }
    if phone(line) {
        kinds.push("a phone number");
    }
    if long_hex(line) {
        kinds.push("a live-looking secret");
    }
    if let Some(secret) = scan_outbound_secret(line) {
        kinds.push(secret);
    }
    let lower = line.to_lowercase();
    if own
        .iter()
        .any(|v| v.chars().count() > 2 && lower.contains(&v.to_lowercase()))
    {
        kinds.push("one of your own values");
    }
    kinds
}

/// The owner's own values from the node's environment: `ENDORA_PERSONAL_VALUES`, comma
/// separated, as `local.mk` holds them; the short ones (two characters or fewer) are dropped,
/// as the repository check drops them.
#[must_use]
pub fn own_values(env: Option<&str>) -> Vec<String> {
    env.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|v| v.chars().count() > 2)
        .map(str::to_owned)
        .collect()
}

/// Each line a unified diff adds, with its file and its line number in the new file.
fn added(diff: &str) -> Vec<(String, usize, &str)> {
    let mut lines = Vec::new();
    let mut file = String::new();
    let mut at = 0usize;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            file = path.strip_prefix("b/").unwrap_or(path).to_owned();
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("diff --git ") {
            continue;
        }
        if let Some(hunk) = line.strip_prefix("@@ ") {
            // `@@ -a,b +c,d @@`: the new file's lines start at c.
            at = hunk
                .split_whitespace()
                .find_map(|p| p.strip_prefix('+'))
                .and_then(|p| p.split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            lines.push((file.clone(), at, text));
            at += 1;
            continue;
        }
        if line.starts_with(' ') {
            at += 1;
        }
    }
    lines
}

/// Each line a unified diff removes, with its file.
fn removed(diff: &str) -> Vec<(String, &str)> {
    let mut lines = Vec::new();
    let mut file = String::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            file = path.strip_prefix("a/").unwrap_or(path).to_owned();
            continue;
        }
        if line.starts_with("+++ ") {
            continue;
        }
        if let Some(text) = line.strip_prefix('-') {
            lines.push((file.clone(), text));
        }
    }
    lines
}

/// The privacy scan of a branch: every line its diff adds, and every commit message, by shape,
/// by the secret tripwire, and against the owner's own values. Each finding names its kind and
/// where it is, never the value; the same kind at the same place is found once.
#[must_use]
pub fn scan(diff: &str, messages: &[(String, String)], own: &[String]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let mut add = |what: &str, at: String| {
        if !found.iter().any(|f| f.what == what && f.at == at) {
            found.push(Found {
                what: what.to_owned(),
                at,
            });
        }
    };
    for (file, line, text) in added(diff) {
        for kind in kinds_in(text, own) {
            add(kind, format!("{file} line {line}"));
        }
    }
    for (sha, message) in messages {
        let short = &sha[..12.min(sha.len())];
        for text in message.lines() {
            for kind in kinds_in(text, own) {
                add(kind, format!("commit {short} message"));
            }
        }
    }
    found
}

/// Every `vault_*` variable a line names.
fn vault_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("vault_").map(|n| n + from) {
        let before = text[..at].chars().next_back();
        let name: String = text[at..].chars().take_while(|c| word(*c)).collect();
        from = at + name.len().max(1);
        if before.is_some_and(word) || name.len() <= "vault_".len() {
            continue;
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The variable an assert's condition requires set: `foo is defined`, `foo | length > 0`,
/// `foo != ""`, `foo is not none`.
fn asserted(condition: &str) -> Option<String> {
    let c = condition
        .trim()
        .trim_start_matches("- ")
        .trim_start_matches("that:")
        .trim();
    // One pair of outer quotes, as YAML writes a condition that starts with a quote.
    let c = ['"', '\'']
        .iter()
        .find_map(|q| c.strip_prefix(*q).and_then(|c| c.strip_suffix(*q)))
        .unwrap_or(c);
    let name: String = c.chars().take_while(|ch| word(*ch)).collect();
    if name.is_empty() || name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let rest: String = c[name.len()..].split_whitespace().collect();
    let sets = ["isdefined", "|length>0", "!=\"\"", "!=''", "isnotnone"]
        .iter()
        .any(|shape| rest.starts_with(shape));
    sets.then_some(name)
}

/// What a change needs of the owner before it is deployed, by code: each `vault_*` variable a
/// role's defaults now read that they did not before, and each variable an assert the change
/// adds requires set. Named with the file, never a value.
#[must_use]
pub fn owner_needs(diff: &str) -> Vec<Found> {
    let mut needs: Vec<Found> = Vec::new();
    let mut need = |what: String, at: &str| {
        if !needs.iter().any(|n| n.what == what && n.at == at) {
            needs.push(Found {
                what,
                at: at.to_owned(),
            });
        }
    };
    let gone = removed(diff);
    let lines = added(diff);
    let is_defaults = |file: &str| {
        file.split('/').any(|part| part == "defaults")
            && (file.ends_with(".yml") || file.ends_with(".yaml"))
    };
    for (file, _, text) in &lines {
        if !is_defaults(file) {
            continue;
        }
        for name in vault_names(text) {
            let before = gone
                .iter()
                .any(|(f, t)| f == file && vault_names(t).contains(&name));
            if !before {
                need(name, file);
            }
        }
    }
    let asserts: Vec<&String> = lines
        .iter()
        .filter(|(_, _, t)| {
            let t = t.trim_start().trim_start_matches("- ");
            t.starts_with("assert:") || t.starts_with("ansible.builtin.assert:")
        })
        .map(|(f, _, _)| f)
        .collect();
    for (file, _, text) in &lines {
        if !asserts.contains(&file) {
            continue;
        }
        if let Some(name) = asserted(text) {
            need(name, file);
        }
    }
    needs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(file: &str, added: &[&str]) -> String {
        let mut d = format!(
            "diff --git a/{file} b/{file}\n--- a/{file}\n+++ b/{file}\n@@ -1,1 +1,{} @@\n same\n",
            added.len() + 1
        );
        for line in added {
            d.push('+');
            d.push_str(line);
            d.push('\n');
        }
        d
    }

    /// The shapes are built at run time, so the repository's own check never reads one here.
    #[test]
    fn each_shape_is_found_with_its_place_and_never_its_value() {
        let host = format!("{}.{}.7.23", 192, 168);
        let mail = format!("jane.roe{}realmail.net", '@');
        let phone = format!("(212) 555-{}", 1234);
        let hex = "0123456789abcdef".repeat(2);
        let lines = [
            format!("drift_host: {host}"),
            format!("drift_mail: {mail}"),
            format!("lat: {}.{}", 40, 71283),
            format!("drift_phone: {phone}"),
            format!("drift_token: {hex}"),
            format!(
                "drift_key: sk-ant-api03-{}",
                "abcdefghijklmnopqrstuvwxyz0123456789"
            ),
        ];
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let d = diff("roles/drift/defaults/main.yml", &lines);
        let found = scan(&d, &[], &[]);
        let at = |n: usize| format!("roles/drift/defaults/main.yml line {n}");
        let kinds: Vec<(&str, String)> = found
            .iter()
            .map(|f| (f.what.as_str(), f.at.clone()))
            .collect();
        assert!(
            kinds.contains(&("a real network address", at(2))),
            "{kinds:?}"
        );
        assert!(kinds.contains(&("a real mailbox", at(3))), "{kinds:?}");
        assert!(kinds.contains(&("coordinates", at(4))), "{kinds:?}");
        assert!(kinds.contains(&("a phone number", at(5))), "{kinds:?}");
        assert!(
            kinds.contains(&("a live-looking secret", at(6))),
            "{kinds:?}"
        );
        assert!(found.iter().any(|f| f.at == at(7)), "{kinds:?}");
        let said = format!("{found:?}");
        for value in [
            host.as_str(),
            "jane.roe",
            "40.71283",
            phone.as_str(),
            "sk-ant",
        ] {
            assert!(!said.contains(value), "a value was said: {said}");
        }
    }

    #[test]
    fn the_examples_the_repository_allows_are_not_found() {
        let d = diff(
            "docs/a.md",
            &[
                "Use 192.168.1.10, 127.0.0.1, 172.18.0.2 or 10.0.0.0 in examples.",
                "Mail john@example.com, hq@endora.invalid, noreply@anthropic.com.",
                "Unit getty@tty1.service; clone git@github.com:owner/name.git",
                "image: foo@sha256:0123456789abcdef0123456789abcdef0123456789abcdef",
                "version 1.2.3, ansible-core 2.15.13, port 8112",
            ],
        );
        assert_eq!(scan(&d, &[], &[]), Vec::new());
    }

    #[test]
    fn only_added_lines_are_read_and_removed_ones_are_leaving() {
        let d = format!(
            "--- a/x.yml\n+++ b/x.yml\n@@ -1,2 +1,1 @@\n-host: {}.{}.7.23\n keep: 1\n",
            192, 168
        );
        assert_eq!(scan(&d, &[], &[]), Vec::new());
    }

    #[test]
    fn the_owners_own_values_are_found_in_any_case_and_in_commit_messages() {
        let own = own_values(Some(" New York , Jo, Jane Roe "));
        assert_eq!(
            own,
            ["New York", "Jane Roe"],
            "two characters or fewer are dropped"
        );
        let d = diff("roles/a/README.md", &["Runs in NEW YORK."]);
        let messages = vec![(
            "a1b2c3d4e5f60718293a".to_owned(),
            "tidy the role\n\nAsked by jane roe".to_owned(),
        )];
        let found = scan(&d, &messages, &own);
        assert_eq!(
            found,
            vec![
                Found {
                    what: "one of your own values".to_owned(),
                    at: "roles/a/README.md line 2".to_owned()
                },
                Found {
                    what: "one of your own values".to_owned(),
                    at: "commit a1b2c3d4e5f6 message".to_owned()
                },
            ]
        );
        assert!(own_values(None).is_empty());
    }

    #[test]
    fn a_role_default_that_now_reads_a_vault_variable_needs_the_owner() {
        let d = diff(
            "roles/drift/defaults/main.yml",
            &[
                "drift_token: \"{{ vault_drift_token }}\"",
                "drift_port: 8080",
            ],
        );
        assert_eq!(
            owner_needs(&d),
            vec![Found {
                what: "vault_drift_token".to_owned(),
                at: "roles/drift/defaults/main.yml".to_owned()
            }]
        );
        // Read before as well (a line moved or reworded): nothing new is needed.
        let moved = "--- a/roles/drift/defaults/main.yml\n+++ b/roles/drift/defaults/main.yml\n@@ -1,1 +1,1 @@\n-drift_token: \"{{ vault_drift_token }}\"\n+drift_token: \"{{ vault_drift_token | default('') }}\"\n";
        assert_eq!(owner_needs(moved), Vec::new());
        // A vault variable outside a role's defaults is the role's business, not a default.
        let tasks = diff(
            "roles/drift/tasks/main.yml",
            &["x: \"{{ vault_drift_token }}\""],
        );
        assert_eq!(owner_needs(&tasks), Vec::new());
    }

    #[test]
    fn an_assert_that_a_variable_is_set_needs_the_owner() {
        let d = diff(
            "roles/drift/tasks/main.yml",
            &[
                "- name: The token is set",
                "  ansible.builtin.assert:",
                "    that:",
                "      - drift_token is defined",
                "      - drift_token | length > 0",
                "      - \"drift_host != ''\"",
                "      - drift_port > 0",
            ],
        );
        let needs: Vec<String> = owner_needs(&d).into_iter().map(|n| n.what).collect();
        assert_eq!(needs, ["drift_token", "drift_host"]);
        // The same lines without an assert added are no ask.
        let plain = diff(
            "roles/drift/tasks/main.yml",
            &["when: drift_token is defined"],
        );
        assert_eq!(owner_needs(&plain), Vec::new());
    }
}
