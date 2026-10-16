//! The brain: what Endora knows about the person, as markdown pages (ADR 0101).
//!
//! Every page is a file. It has a `# Title`, a summary of what is true now, and a
//! `## Timeline` of dated lines that are only ever added to. `memory.md` is the one
//! page read whole on every turn: what the person told Endora, and the little it knows
//! best. `index.md` lists every page with its one line, and `log.md` says what changed.
//!
//! The person can edit any page by hand, so nothing here re-renders a page from a model
//! of it: each change is a small edit to the text, and everything else in the file is
//! kept exactly as it was.

use core::fmt;

/// The page read whole on every turn.
pub const MEMORY: &str = "memory";
/// The section of [`MEMORY`] the person wrote, through settings or by hand.
pub const TOLD: &str = "What you told me";
/// The section of [`MEMORY`] Endora keeps: the little it knows best.
pub const KNOWS: &str = "What I know best";
/// The section of every page that is only ever added to.
pub const TIMELINE: &str = "Timeline";
/// How many lines "What I know best" may hold. Memory is read on every turn, so it
/// stays short; depth belongs on the pages.
pub const MOST_MEMORY_LINES: usize = 30;
/// The longest fact a page takes. A fact is one sentence, not a document.
pub const LONGEST_FACT: usize = 300;

/// Why a page name or a fact was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrainError {
    /// Not a page name this brain accepts, with why.
    BadPath(&'static str),
    /// Not something a page can hold, with why.
    BadFact(&'static str),
}

impl fmt::Display for BrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPath(why) => write!(f, "not a page name: {why}"),
            Self::BadFact(why) => write!(f, "not a fact a page can hold: {why}"),
        }
    }
}

impl std::error::Error for BrainError {}

/// Where a page lives: up to three lowercase segments, such as `hobbies/watches`.
///
/// The name is the file's place in the folder, so it is the one thing that must never
/// reach outside it: only `a-z`, `0-9` and `-`, no dots, no empty segments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PagePath(String);

impl PagePath {
    /// Reads a page name the way a person or a model might write it ("Hobbies/Watches",
    /// "hobbies/watches.md", "home lab/nas") and returns it in its one written form.
    ///
    /// # Errors
    /// [`BrainError::BadPath`] for anything that is not a page of this brain.
    pub fn parse(raw: &str) -> Result<Self, BrainError> {
        let raw = raw.trim().trim_matches('/');
        let raw = raw.strip_suffix(".md").unwrap_or(raw);
        if raw.is_empty() {
            return Err(BrainError::BadPath("empty"));
        }
        let mut segments = Vec::new();
        for segment in raw.split('/') {
            let segment: String = segment
                .trim()
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
            if segment.is_empty() {
                return Err(BrainError::BadPath("an empty folder"));
            }
            if segment.len() > 48 {
                return Err(BrainError::BadPath("a name longer than 48 characters"));
            }
            if !segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(BrainError::BadPath("only letters, digits and hyphens"));
            }
            if segment.starts_with('-') || segment.ends_with('-') {
                return Err(BrainError::BadPath(
                    "a name starting or ending with a hyphen",
                ));
            }
            segments.push(segment);
        }
        if segments.len() > 3 {
            return Err(BrainError::BadPath("more than three levels"));
        }
        let path = segments.join("/");
        if path == "index" || path == "log" {
            return Err(BrainError::BadPath("index and log are Endora's own"));
        }
        Ok(Self(path))
    }

    /// The page read on every turn.
    #[must_use]
    pub fn memory() -> Self {
        Self(MEMORY.to_owned())
    }

    /// The page for a file found in the folder (`hobbies/watches.md`), or `None` for a
    /// file that is not a page: `index.md`, `log.md`, anything not markdown, anything
    /// whose name this brain would not have written.
    #[must_use]
    pub fn from_file(relative: &str) -> Option<Self> {
        let name = relative.strip_suffix(".md")?;
        let path = Self::parse(name).ok()?;
        (path.0 == name).then_some(path)
    }

    /// The name, such as `hobbies/watches`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file, relative to the brain's folder.
    #[must_use]
    pub fn file(&self) -> String {
        format!("{}.md", self.0)
    }

    /// The folder it sits in, or `None` at the top.
    #[must_use]
    pub fn folder(&self) -> Option<&str> {
        self.0.rsplit_once('/').map(|(folder, _)| folder)
    }

    /// Whether this is [`MEMORY`].
    #[must_use]
    pub fn is_memory(&self) -> bool {
        self.0 == MEMORY
    }

    /// A title for a page that does not have one yet: "home-lab/nas" is "Nas".
    #[must_use]
    pub fn title_guess(&self) -> String {
        let last = self
            .0
            .rsplit('/')
            .next()
            .unwrap_or(&self.0)
            .replace('-', " ");
        let mut chars = last.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    }
}

impl fmt::Display for PagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One fact as a page takes it: a single line, trimmed, at most [`LONGEST_FACT`]
/// characters. A line break would let a fact write a heading into the page.
///
/// # Errors
/// [`BrainError::BadFact`] if nothing is left, or it is too long to be one fact.
pub fn one_fact(raw: &str) -> Result<String, BrainError> {
    let fact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let fact = fact
        .trim_start_matches(['#', '-', '*', ' '])
        .trim()
        .to_owned();
    if fact.is_empty() {
        return Err(BrainError::BadFact("empty"));
    }
    if fact.chars().count() > LONGEST_FACT {
        return Err(BrainError::BadFact("longer than one fact"));
    }
    Ok(fact)
}

/// A new page, its summary seeded with its first fact.
#[must_use]
pub fn new_page(title: &str, date: &str, fact: &str) -> String {
    format!("# {title}\n\n{fact}\n\n## {TIMELINE}\n\n- {date}: {fact}\n")
}

/// A new, empty memory.
#[must_use]
pub fn new_memory() -> String {
    format!(
        "# Memory\n\nWho you are, kept short. Endora reads all of this every time.\n\n\
         ## {TOLD}\n\n## {KNOWS}\n"
    )
}

/// The page's title: its first `# ` line, or a guess from its name.
#[must_use]
pub fn title_of(text: &str, path: &PagePath) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map_or_else(|| path.title_guess(), str::to_owned)
}

/// What is true now: the text between the title and the first section.
#[must_use]
pub fn summary_of(text: &str) -> String {
    let mut lines = text.lines().skip_while(|line| !line.starts_with("# "));
    lines.next();
    lines
        .take_while(|line| !line.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// The summary's first line, cut to a length the index can hold.
#[must_use]
pub fn one_line(summary: &str) -> String {
    let first = summary.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let first = first.trim();
    if first.chars().count() <= 120 {
        return first.to_owned();
    }
    let cut: String = first.chars().take(117).collect();
    format!("{}…", cut.trim_end())
}

/// The bullet lines of one `## ` section, without their bullets.
#[must_use]
pub fn section(text: &str, heading: &str) -> Vec<String> {
    let wanted = format!("## {heading}");
    text.lines()
        .skip_while(|line| line.trim() != wanted)
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The text with one `## ` section's bullets set to these lines. Everything outside
/// the section is kept as it was, and so is any line inside it that is not a bullet (a
/// note the person wrote there), after the bullets. A missing section is added at the end.
#[must_use]
pub fn with_section(text: &str, heading: &str, lines: &[String]) -> String {
    let wanted = format!("## {heading}");
    let all: Vec<&str> = text.lines().collect();
    let Some(start) = all.iter().position(|line| line.trim() == wanted) else {
        let body: String = lines.iter().map(|line| format!("- {line}\n")).collect();
        return format!("{}\n\n{wanted}\n\n{body}", text.trim_end());
    };
    let end = all[start + 1..]
        .iter()
        .position(|line| line.starts_with("## "))
        .map_or(all.len(), |i| start + 1 + i);
    let theirs: Vec<&str> = all[start + 1..end]
        .iter()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with("- "))
        .copied()
        .collect();
    let mut out = all[..=start].join("\n");
    out.push_str("\n\n");
    for line in lines {
        out.push_str(&format!("- {line}\n"));
    }
    if !theirs.is_empty() {
        if !lines.is_empty() {
            out.push('\n');
        }
        out.push_str(&theirs.join("\n"));
        out.push('\n');
    }
    if end < all.len() {
        out.push('\n');
        out.push_str(&all[end..].join("\n"));
        out.push('\n');
    }
    out
}

/// The text with the summary replaced. The title and every section are kept.
#[must_use]
pub fn with_summary(text: &str, path: &PagePath, summary: &str) -> String {
    let title = title_of(text, path);
    let rest: Vec<&str> = text
        .lines()
        .skip_while(|line| !line.starts_with("## "))
        .collect();
    let mut out = format!("# {title}\n\n{}\n", summary.trim());
    if !rest.is_empty() {
        out.push('\n');
        out.push_str(&rest.join("\n"));
        out.push('\n');
    }
    out
}

/// The timeline's entries, oldest first, each as written (`2026-10-16: …`).
#[must_use]
pub fn timeline_of(text: &str) -> Vec<String> {
    section(text, TIMELINE)
}

/// The text with a fact added after the last entry of its timeline, or `None` when
/// the timeline already says it: telling Endora the same thing twice is one fact. It is
/// one inserted line; nothing else in the page moves.
#[must_use]
pub fn with_timeline_entry(text: &str, date: &str, fact: &str) -> Option<String> {
    let already = timeline_of(text).iter().any(|entry| {
        let said = entry
            .split_once(": ")
            .map_or(entry.as_str(), |(_, said)| said);
        same_words(said, fact)
    });
    if already {
        return None;
    }
    let entry = format!("- {date}: {fact}");
    let wanted = format!("## {TIMELINE}");
    let all: Vec<&str> = text.lines().collect();
    let Some(start) = all.iter().position(|line| line.trim() == wanted) else {
        return Some(format!("{}\n\n{wanted}\n\n{entry}\n", text.trim_end()));
    };
    let end = all[start + 1..]
        .iter()
        .position(|line| line.starts_with("## "))
        .map_or(all.len(), |i| start + 1 + i);
    let last_entry = all[start + 1..end]
        .iter()
        .rposition(|line| line.trim().starts_with("- "))
        .map(|i| start + 1 + i);
    let mut out: Vec<String> = all.iter().map(|line| (*line).to_owned()).collect();
    match last_entry {
        Some(at) => out.insert(at + 1, entry),
        None => {
            out.insert(start + 1, String::new());
            out.insert(start + 2, entry);
        }
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    Some(joined)
}

/// Whether two lines say the same thing, ignoring case, spacing and punctuation.
#[must_use]
pub fn same_words(a: &str, b: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    words(a) == words(b)
}

/// `YYYY-MM-DD` for a moment in milliseconds since the epoch. The caller adds the
/// person's distance from UTC first when it wants their own date.
#[must_use]
pub fn date_of(ms: i64) -> String {
    // Howard Hinnant's days-to-civil, proleptic Gregorian.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The index: every page, grouped by folder, with its title and one line.
#[must_use]
pub fn index(pages: &[(PagePath, String, String)]) -> String {
    let mut sorted: Vec<&(PagePath, String, String)> =
        pages.iter().filter(|(p, ..)| !p.is_memory()).collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("# Index\n\nEvery page Endora keeps, and what it holds.\n");
    let mut folder: Option<Option<&str>> = None;
    for (path, title, line) in sorted {
        if folder != Some(path.folder()) {
            folder = Some(path.folder());
            out.push_str(&format!("\n## {}\n\n", path.folder().unwrap_or("Pages")));
        }
        if line.is_empty() {
            out.push_str(&format!("- [[{path}]] {title}\n"));
        } else {
            out.push_str(&format!("- [[{path}]] {title}: {line}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> PagePath {
        PagePath::parse(raw).unwrap()
    }

    #[test]
    fn a_page_name_is_read_the_way_people_write_it() {
        assert_eq!(path("Hobbies/Watches").as_str(), "hobbies/watches");
        assert_eq!(path("hobbies/watches.md").as_str(), "hobbies/watches");
        assert_eq!(path(" home lab / NAS ").as_str(), "home-lab/nas");
        assert_eq!(path("memory").file(), "memory.md");
        assert_eq!(path("a/b/c").folder(), Some("a/b"));
        assert_eq!(path("routine").folder(), None);
    }

    #[test]
    fn a_page_name_never_reaches_outside_the_folder() {
        for bad in [
            "",
            "../secrets",
            "a/../../etc/passwd",
            "a//b",
            ".git/config",
            "hobbies\\watches",
            "notes.txt.md/x",
            "a/b/c/d",
            "-a",
            "index",
            "log",
            "caf\u{e9}",
        ] {
            assert!(PagePath::parse(bad).is_err(), "{bad:?} was accepted");
        }
        // A leading slash is not a way out: the name is always read inside the folder.
        assert_eq!(path("/etc/passwd").file(), "etc/passwd.md");
    }

    #[test]
    fn only_files_this_brain_would_write_are_pages() {
        assert_eq!(
            PagePath::from_file("hobbies/watches.md"),
            Some(path("hobbies/watches"))
        );
        assert_eq!(PagePath::from_file("memory.md"), Some(PagePath::memory()));
        for not_a_page in [
            "index.md",
            "log.md",
            "Notes.md",
            "a b.md",
            "readme.txt",
            ".git/x.md",
        ] {
            assert_eq!(PagePath::from_file(not_a_page), None, "{not_a_page}");
        }
    }

    #[test]
    fn a_fact_is_one_line_and_cannot_write_a_heading() {
        assert_eq!(
            one_fact("  collects  dive\nwatches ").unwrap(),
            "collects dive watches"
        );
        assert_eq!(one_fact("## Timeline\n- x").unwrap(), "Timeline - x");
        assert!(one_fact(" \n ").is_err());
        assert!(one_fact(&"a".repeat(LONGEST_FACT + 1)).is_err());
    }

    #[test]
    fn a_new_page_has_a_title_a_summary_and_a_timeline() {
        let page = new_page("Watches", "2026-10-16", "Collects dive watches");
        let p = path("hobbies/watches");
        assert_eq!(title_of(&page, &p), "Watches");
        assert_eq!(summary_of(&page), "Collects dive watches");
        assert_eq!(timeline_of(&page), ["2026-10-16: Collects dive watches"]);
    }

    #[test]
    fn a_fact_goes_on_the_end_of_the_timeline_and_the_rest_is_kept() {
        let page = "# Watches\n\nCollects dive watches.\n\nA note the person wrote.\n\n\
                    ## Timeline\n\n- 2026-10-01: Collects dive watches\n\n## Their links\n\n\
                    - [[hobbies/straps]]\n";
        let after = with_timeline_entry(page, "2026-10-16", "Wants a GMT next").unwrap();
        assert_eq!(
            timeline_of(&after),
            [
                "2026-10-01: Collects dive watches",
                "2026-10-16: Wants a GMT next"
            ]
        );
        assert!(after.contains("A note the person wrote."));
        assert_eq!(section(&after, "Their links"), ["[[hobbies/straps]]"]);
        assert!(after.starts_with("# Watches\n\nCollects dive watches."));
    }

    #[test]
    fn a_line_the_person_wrote_in_a_section_is_never_lost() {
        let page = "# Watches\n\nSummary.\n\n## Timeline\n\n- 2026-10-01: First\n\nMy own note.\n";
        let after = with_timeline_entry(page, "2026-10-16", "Second").unwrap();
        assert!(after.contains("My own note."), "{after}");
        assert_eq!(
            timeline_of(&after),
            ["2026-10-01: First", "2026-10-16: Second"]
        );
        assert!(after.find("Second").unwrap() < after.find("My own note.").unwrap());

        let memory =
            "# Memory\n\n## What you told me\n\nI wrote this myself.\n- Based in: New York\n";
        let after = with_section(memory, TOLD, &["Based in: Boston".to_owned()]);
        assert!(after.contains("I wrote this myself."), "{after}");
        assert_eq!(section(&after, TOLD), ["Based in: Boston"]);
    }

    #[test]
    fn telling_it_the_same_thing_twice_is_one_fact() {
        let page = new_page("Watches", "2026-10-01", "Collects dive watches");
        assert_eq!(
            with_timeline_entry(&page, "2026-10-16", "collects dive watches."),
            None
        );
    }

    #[test]
    fn a_page_without_a_timeline_gets_one() {
        let page = "# Routine\n\nUp at six on weekdays.\n";
        let after = with_timeline_entry(page, "2026-10-16", "Gym on Tuesdays").unwrap();
        assert_eq!(timeline_of(&after), ["2026-10-16: Gym on Tuesdays"]);
        assert_eq!(summary_of(&after), "Up at six on weekdays.");
    }

    #[test]
    fn a_new_summary_keeps_the_title_and_every_section() {
        let page = new_page("Watches", "2026-10-01", "Collects dive watches");
        let p = path("hobbies/watches");
        let after = with_summary(&page, &p, "Collects dive watches; wants a GMT.");
        assert_eq!(summary_of(&after), "Collects dive watches; wants a GMT.");
        assert_eq!(title_of(&after, &p), "Watches");
        assert_eq!(timeline_of(&after), timeline_of(&page));
    }

    #[test]
    fn memory_keeps_what_they_told_it_apart_from_what_it_knows() {
        let memory = new_memory();
        let memory = with_section(
            &memory,
            TOLD,
            &[
                "Based in: New York".to_owned(),
                "Address me as: John".to_owned(),
            ],
        );
        let memory = with_section(&memory, KNOWS, &["Busy Monday to Friday".to_owned()]);
        assert_eq!(
            section(&memory, TOLD),
            ["Based in: New York", "Address me as: John"]
        );
        assert_eq!(section(&memory, KNOWS), ["Busy Monday to Friday"]);
        let emptied = with_section(&memory, TOLD, &[]);
        assert!(section(&emptied, TOLD).is_empty());
        assert_eq!(section(&emptied, KNOWS), ["Busy Monday to Friday"]);
    }

    #[test]
    fn the_index_lists_every_page_but_memory_by_folder() {
        let index = index(&[
            (
                path("hobbies/watches"),
                "Watches".into(),
                "Collects dive watches".into(),
            ),
            (PagePath::memory(), "Memory".into(), "Who you are".into()),
            (path("routine"), "Routine".into(), String::new()),
            (
                path("hobbies/tech"),
                "Tech".into(),
                "Likes small computers".into(),
            ),
        ]);
        assert!(!index.contains("[[memory]]"));
        let tech = index
            .find("[[hobbies/tech]] Tech: Likes small computers")
            .unwrap();
        let watches = index.find("[[hobbies/watches]]").unwrap();
        assert!(tech < watches, "{index}");
        assert!(index.contains("## hobbies"));
        assert!(index.contains("- [[routine]] Routine\n"));
    }

    #[test]
    fn a_date_is_the_calendar_day() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(951_782_400_000), "2000-02-29");
        assert_eq!(date_of(1_792_170_952_163), "2026-10-16");
        assert_eq!(date_of(-1), "1969-12-31");
    }

    #[test]
    fn a_long_summary_is_cut_for_the_index() {
        let line = one_line(&"word ".repeat(60));
        assert!(line.chars().count() <= 120);
        assert!(line.ends_with('…'));
        assert_eq!(one_line("\n\nFirst line.\nSecond."), "First line.");
    }
}
