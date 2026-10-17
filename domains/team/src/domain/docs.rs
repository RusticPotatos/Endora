//! Endora's public records and plans, which the CEO may read (ADR 0111): the decision records
//! in `docs/adr/` and the plans in `docs/plans/`, as they are published. Only those two
//! folders, only Markdown, only a plain file name: never a path that climbs out of them.

use super::work::Refusal;

/// The most characters one read returns: a longer document is read in parts.
pub const DOC_PART: usize = 20_000;

/// The folders the CEO may read.
pub const FOLDERS: [&str; 2] = ["docs/adr", "docs/plans"];

/// What a path to read asks for: one document, or the list of a folder's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked<'a> {
    /// The document at this path.
    Doc(&'a str),
    /// The documents in this folder.
    Folder(&'static str),
}

/// What `path` asks for, if it is one of Endora's public records or plans or their folders:
/// `docs/adr/<name>.md` or `docs/plans/<name>.md`, the name of letters, digits, `.`, `_` and
/// `-` and not starting with a dot.
///
/// # Errors
/// [`Refusal::NotADoc`] for anything else.
pub fn asked(path: &str) -> Result<Asked<'_>, Refusal> {
    let path = path.trim();
    if let Some(folder) = FOLDERS.iter().find(|f| path.trim_end_matches('/') == **f) {
        return Ok(Asked::Folder(folder));
    }
    for folder in FOLDERS {
        let Some(name) = path
            .strip_prefix(folder)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        let plain = !name.starts_with('.')
            && !name.contains("..")
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if plain && name.len() > ".md".len() && name.ends_with(".md") {
            return Ok(Asked::Doc(path));
        }
    }
    Err(Refusal::NotADoc)
}

/// Part `n` (from 1) of a document, at most [`DOC_PART`] characters, and how many parts it has.
/// A part past the last is empty.
#[must_use]
pub fn part(text: &str, n: usize) -> (String, usize) {
    let chars = text.chars().count();
    let parts = chars.div_ceil(DOC_PART).max(1);
    let from = n.saturating_sub(1).saturating_mul(DOC_PART);
    (text.chars().skip(from).take(DOC_PART).collect(), parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_markdown_name_in_the_two_public_folders_is_read() {
        assert_eq!(
            asked("docs/adr/0111-the-ceo-and-the-chain-of-command.md"),
            Ok(Asked::Doc(
                "docs/adr/0111-the-ceo-and-the-chain-of-command.md"
            ))
        );
        assert_eq!(
            asked(" docs/plans/endora-hq.md "),
            Ok(Asked::Doc("docs/plans/endora-hq.md"))
        );
        assert_eq!(asked("docs/adr/"), Ok(Asked::Folder("docs/adr")));
        assert_eq!(asked("docs/plans"), Ok(Asked::Folder("docs/plans")));
        for refused in [
            "docs/adr/../../local.mk",
            "docs/adr/..md",
            "docs/adr/.hidden.md",
            "docs/adr/archive/0001-old.md",
            "docs/adr/0101-the-brain.txt",
            "docs/adr/.md",
            "/etc/passwd",
            "docs/homelab.md",
            "docs/plans/endora-hq/notes.md",
            "docs/adr%2F..%2Fx.md",
            "README.md",
            "",
        ] {
            assert_eq!(asked(refused), Err(Refusal::NotADoc), "{refused}");
        }
    }

    #[test]
    fn a_long_document_is_read_in_parts() {
        let text = "a".repeat(DOC_PART) + "b";
        assert_eq!(part(&text, 1), ("a".repeat(DOC_PART), 2));
        assert_eq!(part(&text, 2), ("b".to_owned(), 2));
        assert_eq!(part(&text, 3), (String::new(), 2));
        assert_eq!(part("short", 1), ("short".to_owned(), 1));
        assert_eq!(part("", 1), (String::new(), 1));
    }
}
