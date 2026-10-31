//! HQ's mechanical checks of work that reaches review (ADR 0142, amended 2026-10-30): what can
//! be judged by rule is judged by HQ, not by a model one rank up, and the reviewer is told the
//! results as facts.
//!
//! - **Quoted evidence** ([`quotes_checked`]): each line the worker's post quotes (in backticks
//!   or a fenced block) and attributes to a receipt (`r12`) is looked for in that receipt's
//!   kept result, whitespace aside. A receipt keeps only the start of its result
//!   ([`super::evidence::EXCERPT_MOST`]); a quote not in a cut excerpt cannot be told apart from
//!   one past the cut, and says so rather than guess.
//! - **Branch scope** ([`scope`]): the files a submitted branch changes, against the paths
//!   nobody touches ([`forbidden`]) and against the paths the task names, if it names any.
//! - **Defaults coverage** ([`coverage`]): when the criteria ask for a README listing every
//!   default, each role README in the branch names every top-level key of the role's
//!   `defaults/main.yml` with its value.

use super::cut::CUT;
use super::record;

/// The header every report starts with: what the reviewer and the thread read.
pub const HEADER: &str =
    "HQ's mechanical checks (measured by HQ from the record, not judged by a model):";

/// A line the worker quoted and the receipt it says it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    /// The receipt's id: `r<id>` as cited.
    pub receipt: i64,
    /// The quoted text.
    pub text: String,
}

/// A receipt as the quotes check reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept<'a> {
    /// Its id.
    pub id: i64,
    /// The tool it called: a backtick-quoted tool name is not a quote of its result.
    pub tool: &'a str,
    /// What it kept of the result.
    pub excerpt: &'a str,
    /// Whether it was taken on the item the quote is checked for: another item's reading
    /// proves nothing of this one.
    pub this_items: bool,
}

/// Whether a quote is in its receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// In the receipt's kept result.
    Found,
    /// Not in it, and the receipt kept the whole result.
    NotFound,
    /// Not in what the receipt kept, which is only the start of its result.
    PastTheCut,
    /// No such receipt in HQ's record.
    NoReceipt,
    /// A receipt of another item's (or of no item's): not evidence for this one.
    NotThisItems,
}

/// The receipts a piece of text names, `r12`, with where each starts.
fn receipts_in(text: &str) -> Vec<(usize, i64)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let starts = bytes[i] == b'r' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric());
        if !starts {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        let ends = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if end > i + 1 && ends {
            if let Ok(id) = text[i + 1..end].parse() {
                found.push((i, id));
            }
        }
        i = end.max(i + 1);
    }
    found
}

/// Whether a quoted span is itself a receipt's name (`r12`), which quotes nothing.
fn is_a_receipt(text: &str) -> bool {
    let t = text.trim();
    t.len() > 1 && t.starts_with('r') && t[1..].chars().all(|c| c.is_ascii_digit())
}

/// The lines a post quotes and attributes to a receipt. A fenced block's lines belong to the
/// receipt its opening fence names (```` ```r12 ````), or else the last one the line before it
/// names. A backtick span belongs to the nearest receipt named before it on its line, or else
/// the first named after it. Quotes with no receipt are not attributed, so not checked.
#[must_use]
pub fn quotes(post: &str) -> Vec<Quote> {
    let mut found = Vec::new();
    let mut fence: Option<Option<i64>> = None;
    let mut before: Option<&str> = None;
    for line in post.lines() {
        let trimmed = line.trim();
        if let Some(info) = trimmed.strip_prefix("```") {
            if fence.is_some() {
                fence = None;
            } else {
                let named = receipts_in(info)
                    .first()
                    .map(|(_, id)| *id)
                    .or_else(|| before.and_then(|b| receipts_in(b).last().map(|(_, id)| *id)));
                fence = Some(named);
            }
            before = Some(line);
            continue;
        }
        if let Some(named) = fence {
            if let Some(receipt) = named.filter(|_| !trimmed.is_empty()) {
                found.push(Quote {
                    receipt,
                    text: trimmed.to_owned(),
                });
            }
            continue;
        }
        if !trimmed.is_empty() {
            before = Some(line);
        }
        let named = receipts_in(line);
        if named.is_empty() {
            continue;
        }
        let mut parts = line.split('`');
        let mut at = parts.next().map_or(0, |p| p.len() + 1);
        while let (Some(span), Some(after)) = (parts.next(), parts.next()) {
            let start = at;
            at += span.len() + 1 + after.len() + 1;
            if span.trim().is_empty() || is_a_receipt(span) {
                continue;
            }
            let receipt = named
                .iter()
                .rev()
                .find(|(pos, _)| *pos < start)
                .or_else(|| named.iter().find(|(pos, _)| *pos > start))
                .map(|(_, id)| *id);
            if let Some(receipt) = receipt {
                found.push(Quote {
                    receipt,
                    text: span.trim().to_owned(),
                });
            }
        }
    }
    found
}

/// Text as a receipt keeps it, with no whitespace at all: a quote and a result are compared
/// whitespace aside.
fn bare(text: &str) -> String {
    record::clean(text)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Whether one quote is in the receipt it names.
#[must_use]
pub fn seen(quote: &Quote, receipts: &[Kept<'_>]) -> Seen {
    let Some(receipt) = receipts.iter().find(|r| r.id == quote.receipt) else {
        return Seen::NoReceipt;
    };
    if !receipt.this_items {
        return Seen::NotThisItems;
    }
    let quoted = quote
        .text
        .trim()
        .trim_end_matches('…')
        .trim_end_matches("...");
    let cut = receipt.excerpt.ends_with(CUT);
    let kept = receipt.excerpt.trim_end_matches(CUT);
    if bare(kept).contains(&bare(quoted)) {
        return Seen::Found;
    }
    if cut {
        return Seen::PastTheCut;
    }
    Seen::NotFound
}

/// Every quote of a post attributed to a receipt, with whether it is there. A backtick span
/// that is the receipt's own tool name is a name, not a quote, and is left out.
#[must_use]
pub fn quotes_checked(post: &str, receipts: &[Kept<'_>]) -> Vec<(Quote, Seen)> {
    quotes(post)
        .into_iter()
        .filter(|q| {
            !receipts
                .iter()
                .any(|r| r.id == q.receipt && r.tool == q.text)
        })
        .map(|q| {
            let s = seen(&q, receipts);
            (q, s)
        })
        .collect()
}

/// A path nobody's branch may touch: `group_vars/`, `host_vars/`, any inventory, anything
/// named for a vault. The node refuses such a branch at submission; this is the same rule,
/// checked again at review so it never rests on one place.
#[must_use]
pub fn forbidden(path: &str) -> bool {
    let lower = path.trim().trim_start_matches("./").to_ascii_lowercase();
    lower.split('/').any(|p| {
        matches!(
            p,
            "group_vars"
                | "host_vars"
                | "inventories"
                | "inventory"
                | "inventory.yml"
                | "inventory.ini"
        ) || p.contains("vault")
    })
}

/// Whether a word reads as a path: a folder (`roles/x/`), a file with an extension
/// (`README.md`, `defaults/main.yml`), anything under `roles/`, or three segments or more.
fn a_path(word: &str) -> bool {
    if word.is_empty() || word.contains("://") || word.starts_with("hq/") {
        return false;
    }
    if !word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '*'))
    {
        return false;
    }
    let last = word.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let extension = last.rsplit_once('.').is_some_and(|(name, ext)| {
        !name.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic()) && !ext.is_empty()
    });
    let segments = word.trim_end_matches('/').split('/').count();
    (word.contains('/') && word.ends_with('/'))
        || extension
        || word.starts_with("roles/")
        || segments >= 3
}

/// The paths a task's words name, in order, each once.
#[must_use]
pub fn named_paths(words: &str) -> Vec<String> {
    let mut named: Vec<String> = Vec::new();
    for raw in words.split_whitespace() {
        let word = raw
            .trim_matches(|c: char| "`'\"()[]{}<>,;:!?".contains(c))
            .trim_end_matches('.')
            .trim_start_matches("./");
        if a_path(word) && !named.iter().any(|n| n == word) {
            named.push(word.to_owned());
        }
    }
    named
}

/// One segment against a pattern's, where `*` stands for any run of characters.
fn segment_matches(pattern: &str, segment: &str) -> bool {
    let Some((head, rest)) = pattern.split_once('*') else {
        return pattern == segment;
    };
    let Some(tail) = segment.strip_prefix(head) else {
        return false;
    };
    (0..=tail.len())
        .filter(|i| tail.is_char_boundary(*i))
        .any(|i| segment_matches(rest, &tail[i..]))
}

/// Whether a changed file is within a named path: the path is a folder it is under, the file
/// itself, or the file's trailing segments (`defaults/main.yml`, `README.md`).
#[must_use]
pub fn within(file: &str, named: &str) -> bool {
    let file: Vec<&str> = file.trim_start_matches("./").split('/').collect();
    let pattern: Vec<&str> = named
        .trim_start_matches("./")
        .trim_end_matches('/')
        .split('/')
        .collect();
    if pattern.len() > file.len() {
        return false;
    }
    let all = |of: &[&str]| pattern.iter().zip(of).all(|(p, s)| segment_matches(p, s));
    all(&file[..pattern.len()]) || all(&file[file.len() - pattern.len()..])
}

/// What a branch touches, against the rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scope {
    /// Every file it changes.
    pub changed: Vec<String>,
    /// The files it must never have touched.
    pub forbidden: Vec<String>,
    /// The paths the task names; empty when it names none.
    pub named: Vec<String>,
    /// The files outside every named path (none when the task names no path).
    pub outside: Vec<String>,
}

/// A branch's changed files against the forbidden paths and the task's named ones.
#[must_use]
pub fn scope(changed: &[String], task_words: &str) -> Scope {
    let named = named_paths(task_words);
    let forbidden_files = changed.iter().filter(|f| forbidden(f)).cloned().collect();
    let outside = if named.is_empty() {
        Vec::new()
    } else {
        changed
            .iter()
            .filter(|f| !named.iter().any(|n| within(f, n)))
            .cloned()
            .collect()
    };
    Scope {
        changed: changed.to_vec(),
        forbidden: forbidden_files,
        named,
        outside,
    }
}

/// The files a diff changes, from its `diff --git a/x b/y` lines: what a branch recorded before
/// the node told HQ its file names is read from its document. Both sides of each, so a file
/// moved out of a forbidden folder is still one the branch touches, and a name git quoted (one
/// outside ASCII, or with a quote in it) as the name it is.
#[must_use]
pub fn changed_in_diff(diff: &str) -> Vec<String> {
    let mut changed: Vec<String> = Vec::new();
    for line in diff.lines() {
        let Some(rest) = line.strip_prefix("diff --git ") else {
            continue;
        };
        let Some((a, b)) = header_sides(rest) else {
            continue;
        };
        for side in [a, b] {
            if !changed.contains(&side) {
                changed.push(side);
            }
        }
    }
    changed
}

/// The two paths of a `diff --git` header after its words, each without its `a/` or `b/`.
fn header_sides(rest: &str) -> Option<(String, String)> {
    let (a, b) = if rest.starts_with('"') {
        let (a, after) = unquoted(rest)?;
        let after = after.strip_prefix(' ')?;
        let b = if after.starts_with('"') {
            unquoted(after)?.0
        } else {
            after.to_owned()
        };
        (a, b)
    } else if let Some(at) = rest.find(" \"b/") {
        (rest[..at].to_owned(), unquoted(&rest[at + 1..])?.0)
    } else {
        // Unquoted on both sides: a name may hold " b/" itself, so the split that gives the
        // same name on both sides is the one, and otherwise (a move) the first.
        let splits: Vec<usize> = rest.match_indices(" b/").map(|(i, _)| i).collect();
        let at = splits
            .iter()
            .copied()
            .find(|&i| rest[..i].strip_prefix("a/") == Some(&rest[i + 3..]))
            .or_else(|| splits.first().copied())?;
        (rest[..at].to_owned(), rest[at + 1..].to_owned())
    };
    Some((
        a.strip_prefix("a/")?.to_owned(),
        b.strip_prefix("b/")?.to_owned(),
    ))
}

/// A name git quoted (`"..."`, C escapes and octal bytes) as it is, and what follows it.
fn unquoted(quoted: &str) -> Option<(String, &str)> {
    let body = quoted.strip_prefix('"')?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((String::from_utf8_lossy(&bytes).into_owned(), &body[i + 1..])),
            '\\' => {
                let (_, e) = chars.next()?;
                let byte = match e {
                    'a' => 7,
                    'b' => 8,
                    't' => b'\t',
                    'n' => b'\n',
                    'v' => 11,
                    'f' => 12,
                    'r' => b'\r',
                    '0'..='7' => {
                        let mut n = u32::from(e) - u32::from('0');
                        for _ in 0..2 {
                            let (_, d) = chars.next()?;
                            n = n * 8 + d.to_digit(8)?;
                        }
                        u8::try_from(n).ok()?
                    }
                    other => u8::try_from(other).ok()?,
                };
                bytes.push(byte);
            }
            c => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    None
}

/// Whether a task's criteria ask for a README that lists every default.
#[must_use]
pub fn asks_for_defaults(criteria: &[&str]) -> bool {
    criteria.iter().any(|c| {
        let c = c.to_ascii_lowercase();
        c.contains("readme") && c.contains("default")
    })
}

/// The role READMEs a branch changes, each with the defaults file beside it:
/// `roles/x/README.md` and `roles/x/defaults/main.yml`.
#[must_use]
pub fn role_readmes(changed: &[String]) -> Vec<(String, String)> {
    changed
        .iter()
        .filter_map(|f| {
            let (dir, name) = f.rsplit_once('/')?;
            if !name.eq_ignore_ascii_case("readme.md") || !dir.starts_with("roles/") {
                return None;
            }
            Some((f.clone(), format!("{dir}/defaults/main.yml")))
        })
        .collect()
}

/// A YAML value as written, without a trailing comment or the quotes around it. `None` for a
/// value on the lines below (a block, a list, `|` or `>`): its name is checked, not its value.
fn plain_value(raw: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    let mut cut_at = raw.len();
    let mut last = ' ';
    for (i, c) in raw.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && last.is_whitespace() => {
                cut_at = i;
                break;
            }
            None => {}
        }
        last = c;
    }
    let value = raw[..cut_at].trim();
    if value.is_empty() || value.starts_with('|') || value.starts_with('>') {
        return None;
    }
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
        .unwrap_or(value);
    Some(unquoted.to_owned())
}

/// The top-level keys of a `defaults/main.yml`, in order, each with its plain value.
#[must_use]
pub fn top_level_defaults(yaml: &str) -> Vec<(String, Option<String>)> {
    yaml.lines()
        .filter(|l| !l.starts_with(char::is_whitespace) && !l.starts_with('#'))
        .filter(|l| !l.starts_with('-') && !l.starts_with("..."))
        .filter_map(|l| {
            let (key, value) = l.split_once(':')?;
            let key = key.trim().trim_matches(|c| c == '"' || c == '\'');
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return None;
            }
            Some((key.to_owned(), plain_value(value)))
        })
        .collect()
}

/// Whether a line names a key as a word of its own.
fn names_key(line: &str, key: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.match_indices(key).any(|(i, _)| {
        let before = line[..i].chars().next_back().is_none_or(|c| !word(c));
        let after = line[i + key.len()..]
            .chars()
            .next()
            .is_none_or(|c| !word(c));
        before && after
    })
}

/// Whether a README line gives a value: as written, whitespace and case aside. An empty value
/// is given as `""`, `''` or the word empty.
fn gives_value(line: &str, value: &str) -> bool {
    let squash = |t: &str| -> String {
        t.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    if value.is_empty() {
        let l = line.to_lowercase();
        return l.contains("\"\"") || l.contains("''") || l.contains("empty");
    }
    squash(line).contains(&squash(value))
}

/// How a role's README covers its defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// The README.
    pub readme: String,
    /// How many top-level defaults the role has.
    pub keys: usize,
    /// Defaults the README never names.
    pub missing: Vec<String>,
    /// Defaults it names without their value: the key and the value it should give.
    pub wrong_value: Vec<(String, String)>,
}

impl Coverage {
    /// Whether the README lists every default with its value.
    #[must_use]
    pub fn whole(&self) -> bool {
        self.missing.is_empty() && self.wrong_value.is_empty()
    }
}

/// How a README covers a role's defaults: every top-level key named, and its value on a line
/// that names it.
#[must_use]
pub fn coverage(readme_path: &str, readme: &str, defaults_yaml: &str) -> Coverage {
    let defaults = top_level_defaults(defaults_yaml);
    let mut missing = Vec::new();
    let mut wrong_value = Vec::new();
    for (key, value) in &defaults {
        let lines: Vec<&str> = readme.lines().filter(|l| names_key(l, key)).collect();
        if lines.is_empty() {
            missing.push(key.clone());
            continue;
        }
        let Some(value) = value else {
            continue;
        };
        if !lines.iter().any(|l| gives_value(l, value)) {
            wrong_value.push((key.clone(), value.clone()));
        }
    }
    Coverage {
        readme: readme_path.to_owned(),
        keys: defaults.len(),
        missing,
        wrong_value,
    }
}

/// What the checks found on one item, ready to be said.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The worker's quotes, each with whether it is in its receipt.
    pub quotes: Vec<(Quote, Seen)>,
    /// The branch checked and its scope, when the item submitted one.
    pub branch: Option<(String, Scope)>,
    /// Each role README's coverage of its defaults, when the criteria ask for it.
    pub coverage: Vec<Coverage>,
    /// A README whose defaults file the branch's record does not hold, so was not checked.
    pub unchecked: Vec<String>,
}

/// A list said in one line, at most `most` items, saying how many more.
fn listed(items: &[String], most: usize) -> String {
    let mut said = items
        .iter()
        .take(most)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > most {
        said.push_str(&format!(" and {} more", items.len() - most));
    }
    said
}

impl Report {
    /// Whether there is nothing to report: no quote, no branch, no README to check.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
            && self.branch.is_none()
            && self.coverage.is_empty()
            && self.unchecked.is_empty()
    }

    /// The files the branch must never have touched.
    #[must_use]
    pub fn forbidden(&self) -> &[String] {
        self.branch
            .as_ref()
            .map_or(&[], |(_, s)| s.forbidden.as_slice())
    }

    /// Whether any check failed.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.quotes
            .iter()
            .any(|(_, s)| matches!(s, Seen::NotFound | Seen::NoReceipt | Seen::NotThisItems))
            || self
                .branch
                .as_ref()
                .is_some_and(|(_, s)| !s.forbidden.is_empty() || !s.outside.is_empty())
            || self.coverage.iter().any(|c| !c.whole())
    }

    /// The report as the thread and the reviewer read it.
    #[must_use]
    pub fn said(&self) -> String {
        let mut told = format!("{HEADER}\n");
        if !self.quotes.is_empty() {
            let found = self
                .quotes
                .iter()
                .filter(|(_, s)| *s == Seen::Found)
                .count();
            told.push_str(&format!(
                "- Quoted evidence: {found} of {} quoted lines are in their receipts.",
                self.quotes.len()
            ));
            for (q, s) in self.quotes.iter().filter(|(_, s)| *s != Seen::Found) {
                let why = match s {
                    Seen::NotFound => "NOT in its result",
                    Seen::PastTheCut => {
                        "not in the start of the result the receipt keeps (cannot tell)"
                    }
                    Seen::NoReceipt => "no such receipt",
                    Seen::NotThisItems => "not this item's receipt",
                    Seen::Found => "",
                };
                told.push_str(&format!(
                    "\n  - r{}: `{}`: {why}",
                    q.receipt,
                    super::cut::cut(&q.text, 120)
                ));
            }
            told.push('\n');
        }
        if let Some((name, s)) = &self.branch {
            told.push_str(&format!(
                "- Branch scope: {name} changes {} file{}.",
                s.changed.len(),
                if s.changed.len() == 1 { "" } else { "s" }
            ));
            if s.forbidden.is_empty() {
                told.push_str(" None is under group_vars/, host_vars/, an inventory or a vault.");
            } else {
                told.push_str(&format!(
                    " FORBIDDEN (never to be touched): {}.",
                    listed(&s.forbidden, 10)
                ));
            }
            if s.named.is_empty() {
                told.push_str(" The task names no paths, so nothing is out of scope by rule.");
            } else if s.outside.is_empty() {
                told.push_str(&format!(
                    " All within the paths the task names ({}).",
                    listed(&s.named, 6)
                ));
            } else {
                told.push_str(&format!(
                    " Outside the paths the task names ({}): {}.",
                    listed(&s.named, 6),
                    listed(&s.outside, 10)
                ));
            }
            told.push('\n');
        }
        for c in &self.coverage {
            if c.whole() {
                told.push_str(&format!(
                    "- Defaults: {} lists all {} defaults with their values.\n",
                    c.readme, c.keys
                ));
                continue;
            }
            let wrong: Vec<String> = c
                .wrong_value
                .iter()
                .map(|(k, v)| format!("{k} (default {v})"))
                .collect();
            told.push_str(&format!(
                "- Defaults: {} covers {} of {} defaults.",
                c.readme,
                c.keys - c.missing.len() - c.wrong_value.len(),
                c.keys
            ));
            if !c.missing.is_empty() {
                told.push_str(&format!(" Not named: {}.", listed(&c.missing, 12)));
            }
            if !wrong.is_empty() {
                told.push_str(&format!(
                    " Named without its value: {}.",
                    listed(&wrong, 12)
                ));
            }
            told.push('\n');
        }
        for readme in &self.unchecked {
            told.push_str(&format!(
                "- Defaults: {readme} not checked: the branch's record holds no defaults file \
                 beside it.\n"
            ));
        }
        told
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept<'a>(id: i64, tool: &'a str, excerpt: &'a str) -> Kept<'a> {
        Kept {
            id,
            tool,
            excerpt,
            this_items: true,
        }
    }

    #[test]
    fn a_backtick_quote_belongs_to_the_receipt_named_before_it_on_its_line() {
        let post = "Checked: r12 shows `web: running (healthy)` and r13 `db: up`.\n\
                    Nothing here `unattributed`.";
        let got = quotes(post);
        assert_eq!(
            got,
            vec![
                Quote {
                    receipt: 12,
                    text: "web: running (healthy)".to_owned()
                },
                Quote {
                    receipt: 13,
                    text: "db: up".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_backtick_quote_before_any_receipt_takes_the_first_after_it() {
        let got = quotes("`lint: 0 failures` (r7)");
        assert_eq!(
            got,
            vec![Quote {
                receipt: 7,
                text: "lint: 0 failures".to_owned()
            }]
        );
    }

    #[test]
    fn a_fenced_block_belongs_to_its_fence_or_the_line_before() {
        let post = "From r21:\n```\nok: [jane-host]\n\nchanged=0\n```\n```r22\nPLAY RECAP\n```\n\
                    ```\nno receipt\n```";
        let got = quotes(post);
        let said: Vec<(i64, &str)> = got.iter().map(|q| (q.receipt, q.text.as_str())).collect();
        assert_eq!(
            said,
            vec![
                (21, "ok: [jane-host]"),
                (21, "changed=0"),
                (22, "PLAY RECAP")
            ]
        );
    }

    #[test]
    fn a_receipts_own_name_in_backticks_is_not_a_quote() {
        assert!(quotes("see `r12`").is_empty());
        assert!(quotes("words like for12 and r12x are no receipts `x`").is_empty());
    }

    #[test]
    fn a_quote_is_found_whitespace_aside() {
        let receipts = [kept(1, "homelab_status", "web running healthy, db  up")];
        let q = Quote {
            receipt: 1,
            text: "db up".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::Found);
        let q = Quote {
            receipt: 1,
            text: "web   running\thealthy".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::Found);
        let q = Quote {
            receipt: 1,
            text: "web running healthy, db up, cache up".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::NotFound);
        let q = Quote {
            receipt: 2,
            text: "db up".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::NoReceipt);
    }

    #[test]
    fn a_quote_not_in_a_cut_excerpt_cannot_be_told() {
        let excerpt = format!("start of a long log{CUT}");
        let receipts = [kept(3, "read_logs", &excerpt)];
        let q = Quote {
            receipt: 3,
            text: "a line further down".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::PastTheCut);
        let q = Quote {
            receipt: 3,
            text: "long log".to_owned(),
        };
        assert_eq!(seen(&q, &receipts), Seen::Found);
    }

    #[test]
    fn a_receipt_of_another_items_is_not_this_items() {
        let mut theirs = kept(4, "homelab_status", "web: up");
        theirs.this_items = false;
        let got = quotes_checked("r4 says `web: up`", &[theirs]);
        assert_eq!(got[0].1, Seen::NotThisItems);
    }

    #[test]
    fn the_receipts_tool_name_in_backticks_is_left_out() {
        let receipts = [kept(4, "homelab_status", "web: up")];
        let got = quotes_checked("`homelab_status` r4 says `web: up`", &receipts);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, Seen::Found);
    }

    #[test]
    fn forbidden_paths_are_the_owners_values() {
        for p in [
            "group_vars/all.yml",
            "host_vars/nas.yml",
            "inventories/home/hosts",
            "roles/x/files/vault.yml",
            "./inventory",
        ] {
            assert!(forbidden(p), "{p}");
        }
        assert!(!forbidden("roles/jellyfin/defaults/main.yml"));
    }

    #[test]
    fn a_task_names_paths_and_folders_but_not_branches_or_urls() {
        let named = named_paths(
            "Write `roles/jellyfin/README.md` from roles/jellyfin/defaults/main.yml; touch docs/ \
             only. Branch hq/task-7-x, see https://example.com/a/b/c and and/or.",
        );
        assert_eq!(
            named,
            vec![
                "roles/jellyfin/README.md",
                "roles/jellyfin/defaults/main.yml",
                "docs/"
            ]
        );
    }

    #[test]
    fn a_file_is_within_a_folder_a_file_or_a_trailing_path() {
        assert!(within("roles/a/README.md", "roles/a/"));
        assert!(within("roles/a/README.md", "roles/a/README.md"));
        assert!(within("roles/a/README.md", "README.md"));
        assert!(within("roles/a/README.md", "roles/*/README.md"));
        assert!(within("roles/a/tasks/main.yml", "roles/*/tasks/*.yml"));
        assert!(!within("roles/b/README.md", "roles/a/"));
        assert!(!within("docs/a.md", "roles/"));
    }

    #[test]
    fn scope_flags_forbidden_files_and_those_outside_the_named_paths() {
        let changed: Vec<String> = ["roles/a/README.md", "group_vars/all.yml", "docs/x.md"]
            .map(str::to_owned)
            .to_vec();
        let s = scope(&changed, "Document roles/a/ in its README.md");
        assert_eq!(s.forbidden, vec!["group_vars/all.yml"]);
        assert_eq!(s.outside, vec!["group_vars/all.yml", "docs/x.md"]);
        let s = scope(&changed, "Tidy whatever needs it");
        assert!(s.named.is_empty() && s.outside.is_empty());
    }

    #[test]
    fn a_diff_says_its_files() {
        let diff = "diff --git a/roles/a/README.md b/roles/a/README.md\n+x\n\
                    diff --git a/old.md b/new.md\n";
        assert_eq!(
            changed_in_diff(diff),
            vec!["roles/a/README.md", "old.md", "new.md"]
        );
    }

    /// A move out of `group_vars/` is a change to it, and a name git quoted (one outside
    /// ASCII, or with a quote in it) is read as the name it is.
    #[test]
    fn a_diff_says_both_sides_of_a_move_and_quoted_names_unquoted() {
        let diff = "diff --git a/group_vars/all.yml b/roles/all.yml\n\
                    similarity index 100%\n\
                    rename from group_vars/all.yml\n\
                    rename to roles/all.yml\n\
                    diff --git \"a/group_vars/\\303\\251.yml\" \"b/group_vars/\\303\\251.yml\"\n\
                    +x\n\
                    diff --git \"a/docs/say \\\"hi\\\".md\" \"b/docs/say \\\"hi\\\".md\"\n";
        let changed = changed_in_diff(diff);
        assert_eq!(
            changed,
            vec![
                "group_vars/all.yml",
                "roles/all.yml",
                "group_vars/\u{e9}.yml",
                "docs/say \"hi\".md"
            ]
        );
        let s = scope(&changed, "");
        assert_eq!(
            s.forbidden,
            vec!["group_vars/all.yml", "group_vars/\u{e9}.yml"]
        );
    }

    #[test]
    fn the_criteria_ask_for_defaults_when_they_speak_of_a_readme_and_defaults() {
        assert!(asks_for_defaults(&[
            "The README lists every default with its value"
        ]));
        assert!(!asks_for_defaults(&["The lint passes", "README exists"]));
    }

    #[test]
    fn role_readmes_are_paired_with_their_defaults() {
        let changed = [
            "roles/a/README.md",
            "docs/README.md",
            "roles/b/tasks/main.yml",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            role_readmes(&changed),
            vec![(
                "roles/a/README.md".to_owned(),
                "roles/a/defaults/main.yml".to_owned()
            )]
        );
    }

    const DEFAULTS: &str = "---\n# Jellyfin\njellyfin_enabled: false\n\
        jellyfin_port: \"8096\"  # the web port\njellyfin_data: '{{ docker_home }}/jellyfin'\n\
        jellyfin_name: \"\"\njellyfin_env:\n  TZ: UTC\njellyfin_note: \"a # inside\"\n";

    #[test]
    fn top_level_defaults_ignore_comments_quotes_and_nested_keys() {
        let got = top_level_defaults(DEFAULTS);
        let want: Vec<(String, Option<String>)> = vec![
            ("jellyfin_enabled".into(), Some("false".into())),
            ("jellyfin_port".into(), Some("8096".into())),
            (
                "jellyfin_data".into(),
                Some("{{ docker_home }}/jellyfin".into()),
            ),
            ("jellyfin_name".into(), Some(String::new())),
            ("jellyfin_env".into(), None),
            ("jellyfin_note".into(), Some("a # inside".into())),
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn a_readme_that_lists_every_default_with_its_value_is_whole() {
        let readme = "| Variable | Default |\n|---|---|\n| `jellyfin_enabled` | `False` |\n\
            | `jellyfin_port` | `\"8096\"` |\n| `jellyfin_data` | `{{docker_home}}/jellyfin` |\n\
            | `jellyfin_name` | `\"\"` |\n| `jellyfin_env` | a map |\n\
            | `jellyfin_note` | a # inside |\n";
        let c = coverage("roles/jellyfin/README.md", readme, DEFAULTS);
        assert!(c.whole(), "{c:?}");
        assert_eq!(c.keys, 6);
    }

    #[test]
    fn a_readme_missing_a_default_or_its_value_says_which() {
        let readme = "- jellyfin_enabled: false\n- jellyfin_port: 8920\n- jellyfin_data_dir: x\n\
            - jellyfin_name: empty\n- jellyfin_env\n- jellyfin_note: a # inside\n";
        let c = coverage("roles/jellyfin/README.md", readme, DEFAULTS);
        assert_eq!(c.missing, vec!["jellyfin_data"]);
        assert_eq!(
            c.wrong_value,
            vec![("jellyfin_port".to_owned(), "8096".to_owned())]
        );
    }

    #[test]
    fn the_report_says_each_check_as_a_fact() {
        let report = Report {
            quotes: vec![
                (
                    Quote {
                        receipt: 12,
                        text: "web: up".into(),
                    },
                    Seen::Found,
                ),
                (
                    Quote {
                        receipt: 12,
                        text: "db: up".into(),
                    },
                    Seen::NotFound,
                ),
            ],
            branch: Some((
                "hq/task-7-readme".into(),
                scope(
                    &[
                        "roles/a/README.md".to_owned(),
                        "host_vars/nas.yml".to_owned(),
                    ],
                    "roles/a/",
                ),
            )),
            coverage: vec![Coverage {
                readme: "roles/a/README.md".into(),
                keys: 3,
                missing: vec!["a_port".into()],
                wrong_value: vec![],
            }],
            unchecked: vec![],
        };
        let said = report.said();
        assert!(said.starts_with(HEADER), "{said}");
        assert!(
            said.contains("1 of 2 quoted lines are in their receipts"),
            "{said}"
        );
        assert!(said.contains("r12: `db: up`: NOT in its result"), "{said}");
        assert!(
            said.contains("FORBIDDEN (never to be touched): host_vars/nas.yml"),
            "{said}"
        );
        assert!(said.contains("Outside the paths the task names (roles/a/): host_vars/nas.yml"));
        assert!(
            said.contains("covers 2 of 3 defaults. Not named: a_port."),
            "{said}"
        );
        assert!(report.failed());
        assert_eq!(report.forbidden(), ["host_vars/nas.yml".to_owned()]);
        assert!(Report::default().is_empty());
    }
}
