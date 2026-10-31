//! Which lines of the team's world a turn on an item needs (ADR 0142, amended 2026-10-30): the
//! lines that name what the item names (its project, the services, roles and paths in its title,
//! description and criteria). Everything else a briefing leaves to the tool that reads it.
//!
//! Words are compared, not meanings: a line is about the item when it shares a distinctive word
//! with it. A word is not distinctive when it is a common one ([`COMMON`]) or when it is on so
//! many of the lines weighed that it tells them apart from nothing (a criterion's "healthy" on
//! every loop note, the project's "nas" on every house section).

/// Words too common to tell what a line is about: the language's own, and the lab's every-day
/// words, which name no one thing.
const COMMON: [&str; 98] = [
    "about",
    "after",
    "again",
    "all",
    "also",
    "and",
    "any",
    "app",
    "apps",
    "are",
    "back",
    "been",
    "before",
    "both",
    "but",
    "can",
    "check",
    "come",
    "config",
    "container",
    "day",
    "default",
    "does",
    "done",
    "each",
    "every",
    "file",
    "find",
    "fix",
    "for",
    "from",
    "has",
    "have",
    "how",
    "into",
    "its",
    "just",
    "lab",
    "line",
    "list",
    "lists",
    "main",
    "make",
    "may",
    "more",
    "most",
    "must",
    "new",
    "not",
    "now",
    "off",
    "old",
    "one",
    "only",
    "other",
    "our",
    "out",
    "own",
    "readme",
    "role",
    "run",
    "running",
    "same",
    "see",
    "set",
    "should",
    "show",
    "some",
    "sure",
    "task",
    "than",
    "that",
    "the",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "this",
    "time",
    "two",
    "use",
    "value",
    "was",
    "way",
    "week",
    "were",
    "what",
    "when",
    "where",
    "which",
    "while",
    "who",
    "why",
    "will",
    "with",
    "yml",
];

/// The shortest word that can name a thing.
const SHORTEST: usize = 3;

/// A word as compared: lowercase, without a possessive or a plural's `s`.
fn stem(word: &str) -> String {
    let word = word.to_lowercase();
    let word = word.strip_suffix("'s").unwrap_or(&word);
    let word = word.strip_suffix('\u{2019}').unwrap_or(word);
    match word.strip_suffix('s') {
        Some(single) if single.len() >= SHORTEST && !single.ends_with('s') => single.to_owned(),
        _ => word.to_owned(),
    }
}

/// A text's words, stemmed: each whole word (`gluetun_provider`, `ansible-nas`) and its parts.
fn words(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut add = |w: &str| {
        let w = stem(w);
        if w.len() >= SHORTEST && !COMMON.contains(&w.as_str()) && !found.contains(&w) {
            found.push(w);
        }
    };
    for whole in text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '\''))) {
        let whole = whole.trim_matches(|c| matches!(c, '-' | '\'' | '_'));
        add(whole);
        if whole.contains(['_', '-']) {
            for part in whole.split(['_', '-']) {
                add(part);
            }
        }
    }
    found
}

/// What an item names, to weigh the world's lines against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relevance {
    terms: Vec<String>,
}

impl Relevance {
    /// The distinctive words of an item's texts: its title, description, criteria, project.
    #[must_use]
    pub fn of_item(texts: &[&str]) -> Self {
        let mut terms: Vec<String> = Vec::new();
        for text in texts {
            for w in words(text) {
                if !terms.contains(&w) {
                    terms.push(w);
                }
            }
        }
        Self { terms }
    }

    /// Which of `lines` are about the item, in order. A word of the item's on more than a third
    /// of the lines, and on three or more, tells none of them apart, so it is not weighed.
    #[must_use]
    pub fn among(&self, lines: &[&str]) -> Vec<bool> {
        let said: Vec<Vec<String>> = lines.iter().map(|l| words(l)).collect();
        let on = |term: &String| said.iter().filter(|w| w.contains(term)).count();
        let telling: Vec<&String> = self
            .terms
            .iter()
            .filter(|t| {
                let n = on(t);
                n < 3 || n * 3 <= lines.len()
            })
            .collect();
        said.iter()
            .map(|w| telling.iter().any(|t| w.contains(t)))
            .collect()
    }

    /// `items` that are about the item, by what `said` shows of each, in order, and how many
    /// were left out.
    #[must_use]
    pub fn keep<T>(&self, items: Vec<T>, said: impl Fn(&T) -> String) -> (Vec<T>, usize) {
        let lines: Vec<String> = items.iter().map(said).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let about = self.among(&lines);
        let all = items.len();
        let kept: Vec<T> = items
            .into_iter()
            .zip(about)
            .filter(|(_, about)| *about)
            .map(|(item, _)| item)
            .collect();
        let left = all - kept.len();
        (kept, left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: [&str; 6] = [
        "gluetun's provider settings are in roles/gluetun/defaults/main.yml",
        "The backup schedule is in roles/restic/defaults/main.yml",
        "jellyfin's hardware transcoding is set in roles/jellyfin/templates/encoding.xml.j2",
        "Pinned images are each role's <role>_image_tag in defaults/main.yml",
        "traefik's labels for every app come from roles/traefik/templates/labels.j2",
        "ntfy's topic for alerts is set in roles/ntfy/defaults/main.yml",
    ];

    #[test]
    fn a_line_naming_what_the_item_names_is_about_it() {
        let item = Relevance::of_item(&[
            "Find what stops gluetun",
            "Fix it in roles/gluetun and document every default in its README.",
            "gluetun is running and healthy",
            "ansible-nas",
        ]);
        assert_eq!(
            item.among(&MAP),
            vec![true, false, false, false, false, false]
        );
    }

    #[test]
    fn a_word_on_most_lines_tells_nothing_apart() {
        let house = [
            "the nas runs restic",
            "the nas runs jellyfin",
            "the nas runs traefik",
            "gluetun is the vpn",
        ];
        // "nas" is on three of four lines: it tells none apart.
        let item = Relevance::of_item(&["Restart the nas"]);
        assert_eq!(item.among(&house), vec![false; 4]);
        let item = Relevance::of_item(&["Pin restic on the nas", "ansible-nas"]);
        assert_eq!(item.among(&house), vec![true, false, false, false]);
        // On few of many lines, the same word tells them apart.
        let item = Relevance::of_item(&["Tidy the templates"]);
        assert_eq!(
            item.among(&MAP),
            vec![false, false, true, false, true, false]
        );
    }

    #[test]
    fn plurals_and_possessives_are_the_same_word() {
        let item = Relevance::of_item(&["Pin the jellyfin images"]);
        let found = item.among(&MAP);
        assert!(found[2], "jellyfin's is jellyfin");
        assert!(found[3], "images is image: {found:?}");
    }

    #[test]
    fn common_words_are_never_what_a_line_is_about() {
        let item = Relevance::of_item(&["Check that every app is set in the lab"]);
        assert_eq!(item.among(&MAP), vec![false; 6]);
    }

    #[test]
    fn what_is_kept_is_what_is_about_the_item_and_the_rest_is_counted() {
        let item = Relevance::of_item(&["Find what stops gluetun"]);
        let (kept, left) = item.keep(MAP.to_vec(), |l| (*l).to_owned());
        assert_eq!((kept, left), (vec![MAP[0]], 5));
    }

    #[test]
    fn an_item_with_no_distinctive_word_finds_nothing() {
        let item = Relevance::of_item(&["", "Fix it."]);
        assert_eq!(item.among(&MAP), vec![false; 6]);
    }
}
