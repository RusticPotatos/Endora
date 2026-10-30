//! The team's map (ADR 0126): the world has a page for every area, project, goal and role, each
//! with a steward worked out from the record; every briefing opens with a short map of the
//! pages that matter to its member; a member who looks and finds nothing says so, apart from the
//! broken roads code finds; and a nightly quiz on the free model tests that the map carries.
//!
//! This module is the rules, as plain data: page addresses, who stewards what, how a map is
//! composed within its size, how an address cited in a page is found, and the shapes of a
//! proposal and a gap.

use super::ProjectStatus;
use super::brain::Brain;
use super::situations::Weight;
use super::work::{GoalId, ProjectId};

/// The most characters of a map's lines for a turn on the NAS's model.
pub const MAP_CHARS_LOCAL: usize = 1_400;
/// The most characters of a map's lines for a turn on the owner's Mac.
pub const MAP_CHARS_MAC: usize = 1_900;
/// The most characters of a map's lines for a turn on Claude.
pub const MAP_CHARS: usize = 2_400;

/// The most characters of a map's lines for a turn thinking with `brain`: each of the house's
/// two models has its own, by the context it runs (ADR 0130, amended).
#[must_use]
pub const fn chars_for(brain: Brain) -> usize {
    match brain {
        Brain::Local => MAP_CHARS_LOCAL,
        Brain::Mac => MAP_CHARS_MAC,
        Brain::Sonnet | Brain::Haiku | Brain::Opus | Brain::DeepSeek => MAP_CHARS,
    }
}
/// The most characters of one line of a map.
pub const MAP_LINE_MOST: usize = 190;
/// The most sections of one page a map points at.
pub const MAP_SECTIONS_PER_PAGE: usize = 3;
/// The line that ends a map in a briefing; the Claude Code worker gives the block back to a
/// turn whose conversation was compacted.
pub const MAP_END: &str = "(End of your map.)";

/// A page of the world, by the data it is about. Its slug is its address: `world:<slug>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKey {
    /// What the team knows about the house; the owner's.
    House,
    /// How the lab is built, derived from code.
    Lab,
    /// Who is on the team, derived from the record.
    Team,
    /// A project.
    Project(ProjectId),
    /// A goal.
    Goal(GoalId),
}

impl PageKey {
    /// Its slug: `house`, `lab`, `team`, `project/2`, `goal/1`.
    #[must_use]
    pub fn slug(self) -> String {
        match self {
            Self::House => "house".to_owned(),
            Self::Lab => "lab".to_owned(),
            Self::Team => "team".to_owned(),
            Self::Project(p) => format!("project/{}", p.0),
            Self::Goal(g) => format!("goal/{}", g.0),
        }
    }

    /// The page with this slug, in the one form [`slug`](Self::slug) writes it.
    #[must_use]
    pub fn parse(slug: &str) -> Option<Self> {
        let found = match slug.split_once('/') {
            None => match slug {
                "house" => Self::House,
                "lab" => Self::Lab,
                "team" => Self::Team,
                _ => return None,
            },
            Some((kind, rest)) => {
                let number = || rest.parse::<i64>().ok();
                match kind {
                    "project" => Self::Project(ProjectId(number()?)),
                    "goal" => Self::Goal(GoalId(number()?)),
                    _ => return None,
                }
            }
        };
        (found.slug() == slug).then_some(found)
    }

    /// What kind of page it is, as a word.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::House => "house",
            Self::Lab => "lab",
            Self::Team => "team",
            Self::Project(_) => "project",
            Self::Goal(_) => "goal",
        }
    }

    /// Whether HQ derives it from code and the record each time it is read.
    #[must_use]
    pub const fn derived(self) -> bool {
        matches!(self, Self::Lab | Self::Team)
    }

    /// How long an insight or canon on it stays confirmed without a fresh reading: ninety days,
    /// and none for a derived page.
    #[must_use]
    pub const fn horizon_days(self) -> i64 {
        match self {
            Self::Lab | Self::Team => 0,
            Self::House | Self::Project(_) | Self::Goal(_) => 90,
        }
    }
}

/// Who keeps a page (ADR 0142): the owner writes the written pages, and HQ's code derives the
/// rest; nobody else writes the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steward {
    /// The owner.
    Owner,
    /// HQ's code, which derives the page: nobody writes it.
    Hq,
}

impl Steward {
    /// For a person to read: `the owner`, `HQ`.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Owner => "the owner",
            Self::Hq => "HQ",
        }
    }
}

/// Who keeps a page: HQ the derived ones, the owner every other.
#[must_use]
pub const fn steward_of(key: PageKey) -> Steward {
    if key.derived() {
        Steward::Hq
    } else {
        Steward::Owner
    }
}

/// Whether a project's page is worth a place on a map: it is being worked or held.
#[must_use]
pub const fn project_is_current(status: ProjectStatus) -> bool {
    matches!(status, ProjectStatus::Active | ProjectStatus::Paused)
}

// ---- the map -----------------------------------------------------------------------------

/// One line of a map: an address and what it says in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `world:area/3` or `world:area/3#backups`.
    pub address: String,
    /// What it says, in a line.
    pub line: String,
    /// How much may be left off.
    pub weight: Weight,
}

impl Entry {
    /// An entry; its line is cut to [`MAP_LINE_MOST`] characters.
    #[must_use]
    pub fn new(address: impl Into<String>, line: &str, weight: Weight) -> Self {
        let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = if line.chars().count() > MAP_LINE_MOST {
            let cut: String = line.chars().take(MAP_LINE_MOST - 1).collect();
            format!("{}\u{2026}", cut.trim_end())
        } else {
            line
        };
        Self {
            address: address.into(),
            line,
            weight,
        }
    }

    fn said(&self) -> String {
        format!("- {} \u{2014} {}\n", self.address, self.line)
    }
}

/// A map composed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapText {
    /// The block a briefing carries, from its first line to [`MAP_END`].
    pub text: String,
    /// The addresses of the entries kept, in order.
    pub kept: Vec<String>,
    /// The addresses left off to keep it short, named at the end of the block.
    pub left_off: Vec<String>,
}

const MAP_HEADER: &str =
    "Your map of the team's world (read_world <address>; a section is world:<page>#<section>):\n";

/// The map's lines within `cap` characters: the optional entries are left off first, then the
/// recommended, the last first each time; never a core one. What is left off is named, and how
/// to say a thing is missing.
#[must_use]
pub fn compose(entries: Vec<Entry>, cap: usize) -> MapText {
    let mut kept = vec![true; entries.len()];
    let footer = |kept: &[bool]| {
        let off: Vec<&str> = entries
            .iter()
            .zip(kept)
            .filter(|(_, k)| !**k)
            .map(|(e, _)| e.address.as_str())
            .collect();
        let mut text = String::new();
        if !off.is_empty() {
            text.push_str(&format!(
                "Left off this map to keep it short: {} (read_world, or world:team for who \
                 does what).\n",
                off.join(", ")
            ));
        }
        text.push_str("Found where something lives that is not here? Add it with add_to_map.\n");
        text.push_str(MAP_END);
        text.push('\n');
        text
    };
    let size = |kept: &[bool]| -> usize {
        MAP_HEADER.chars().count()
            + footer(kept).chars().count()
            + entries
                .iter()
                .zip(kept)
                .filter(|(_, k)| **k)
                .map(|(e, _)| e.said().chars().count())
                .sum::<usize>()
    };
    while size(&kept) > cap {
        let next = [Weight::Optional, Weight::Recommended]
            .into_iter()
            .find_map(|weight| {
                entries
                    .iter()
                    .enumerate()
                    .rfind(|(i, e)| kept[*i] && e.weight == weight)
                    .map(|(i, _)| i)
            });
        let Some(next) = next else {
            break;
        };
        kept[next] = false;
    }
    let mut text = String::from(MAP_HEADER);
    let mut names = Vec::new();
    for (e, _) in entries.iter().zip(&kept).filter(|(_, k)| **k) {
        text.push_str(&e.said());
        names.push(e.address.clone());
    }
    text.push_str(&footer(&kept));
    let left_off = entries
        .iter()
        .zip(&kept)
        .filter(|(_, k)| !**k)
        .map(|(e, _)| e.address.clone())
        .collect();
    MapText {
        text,
        kept: names,
        left_off,
    }
}

/// The block of a briefing that is the map: its first line to [`MAP_END`], if it has one.
#[must_use]
pub fn map_block(briefing: &str) -> Option<String> {
    let start = briefing.find("Your map of the team's world")?;
    let rest = &briefing[start..];
    let mut end = rest.find(MAP_END)? + MAP_END.len();
    if rest[end..].starts_with('\n') {
        end += 1;
    }
    Some(rest[..end].to_owned())
}

/// A thing on a member's map as plain facts: for the line the briefing shows, and for the
/// questions the nightly quiz asks of it, which code scores against these same facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// `world:house`, or `world:house#backups` for a section.
    pub address: String,
    /// The page's slug.
    pub page: String,
    /// The page's title, or the section's heading.
    pub title: String,
    /// What the page says, as a phrase: `how the lab is built`.
    pub topic: String,
    /// Who keeps it, by name.
    pub steward: String,
    /// Whether it is a section, not a page.
    pub section: bool,
    /// The line a map shows of it.
    pub line: String,
    /// How much may be left off.
    pub weight: Weight,
}

impl Fact {
    /// A page.
    #[must_use]
    pub fn page(key: PageKey, title: &str, topic: &str, steward: String, weight: Weight) -> Self {
        Self {
            address: format!("world:{}", key.slug()),
            page: key.slug(),
            title: title.to_owned(),
            topic: topic.to_owned(),
            line: format!("{title}: {topic}. Kept by {steward}."),
            steward,
            section: false,
            weight,
        }
    }

    /// A section of a page: its heading, and what it says in a line, with how established it is.
    #[must_use]
    pub fn section(
        key: PageKey,
        anchor: &str,
        heading: &str,
        headline: &str,
        stage: &str,
        weight: Weight,
    ) -> Self {
        Self {
            address: format!("world:{}#{anchor}", key.slug()),
            page: key.slug(),
            title: heading.to_owned(),
            topic: headline.to_owned(),
            steward: String::new(),
            section: true,
            line: format!("{heading}: {headline} [{stage}]"),
            weight,
        }
    }

    /// The map's line for it.
    #[must_use]
    pub fn entry(&self) -> Entry {
        Entry::new(self.address.clone(), &self.line, self.weight)
    }
}

/// The briefing without its map block: what a test of what comes first compares.
#[must_use]
pub fn without_map(briefing: &str) -> String {
    match map_block(briefing) {
        Some(block) => briefing.replacen(&block, "", 1),
        None => briefing.to_owned(),
    }
}

// ---- addresses cited in a page ------------------------------------------------------------

/// The addresses a text cites: every `world:…` and `story:…` word in it, lowercase, once each,
/// in the order they first appear. Whether they resolve is for the record to say.
#[must_use]
pub fn cited(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut found: Vec<(usize, String)> = Vec::new();
    for prefix in ["world:", "story:"] {
        for (at, _) in lower.match_indices(prefix) {
            let before = lower[..at].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '/') {
                continue;
            }
            let rest = &lower[at..];
            let end = rest
                .find(|c: char| {
                    !(c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '#' | '-' | '_'))
                })
                .unwrap_or(rest.len());
            let word = rest[..end].trim_end_matches(['-', '_', ':', '#', '/']);
            if word.len() > prefix.len() && !found.iter().any(|(_, w)| w == word) {
                found.push((at, word.to_owned()));
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, w)| w).collect()
}

// ---- proposals and gaps -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macs_map_is_between_the_nas_models_and_claudes_and_each_model_has_its_own() {
        let [nas, mac, claude] = [Brain::Local, Brain::Mac, Brain::Sonnet].map(chars_for);
        assert!(nas < mac && mac < claude, "{nas} < {mac} < {claude}");
        assert_eq!(chars_for(Brain::Local), MAP_CHARS_LOCAL);
        assert_eq!(chars_for(Brain::Mac), MAP_CHARS_MAC);
        for claude in [Brain::Sonnet, Brain::Haiku, Brain::Opus] {
            assert_eq!(chars_for(claude), MAP_CHARS, "{claude:?}");
        }
    }

    #[test]
    fn a_page_has_one_slug_and_reads_back() {
        for key in [
            PageKey::House,
            PageKey::Lab,
            PageKey::Team,
            PageKey::Project(ProjectId(2)),
            PageKey::Goal(GoalId(1)),
        ] {
            assert_eq!(PageKey::parse(&key.slug()), Some(key), "{}", key.slug());
        }
        for wrong in [
            "",
            "area/3",
            "role/developer",
            "project/x",
            "project/03",
            "garage",
            "house/1",
        ] {
            assert_eq!(PageKey::parse(wrong), None, "{wrong}");
        }
        assert!(PageKey::Lab.derived() && PageKey::Team.derived() && !PageKey::House.derived());
        assert_eq!(PageKey::Project(ProjectId(1)).horizon_days(), 90);
    }

    #[test]
    fn a_map_drops_what_matters_least_first_names_it_and_never_drops_a_core_line() {
        let long = "a line of some length ".repeat(7);
        let entry = |address: &str, weight| Entry::new(address, &long, weight);
        let entries = vec![
            entry("world:role/developer", Weight::Core),
            entry("world:area/2", Weight::Core),
            entry("world:house", Weight::Recommended),
            entry("world:project/1", Weight::Recommended),
            entry("world:area/9", Weight::Optional),
            entry("world:area/8", Weight::Optional),
        ];
        let whole = compose(entries.clone(), 10_000);
        assert!(whole.left_off.is_empty() && whole.kept.len() == 6);
        assert!(whole.text.starts_with("Your map of the team's world"));
        assert!(whole.text.trim_end().ends_with(MAP_END));
        assert!(whole.text.contains("add_to_map"));
        assert!(!whole.text.contains("Left off"));
        let tight = compose(entries.clone(), whole.text.chars().count() - 40);
        assert_eq!(
            tight.left_off,
            ["world:area/8"],
            "the last optional goes first"
        );
        assert!(
            tight
                .text
                .contains("Left off this map to keep it short: world:area/8")
        );
        let tighter = compose(entries.clone(), 1);
        assert_eq!(tighter.kept, ["world:role/developer", "world:area/2"]);
        assert_eq!(tighter.left_off.len(), 4);
        assert_eq!(
            map_block(&format!("You are X.\n{}rest", whole.text)),
            Some(whole.text)
        );
    }

    #[test]
    fn an_entrys_line_is_cut_short_and_on_one_line() {
        let long = "word ".repeat(100);
        let e = Entry::new("world:house", &format!("a\n{long}"), Weight::Core);
        assert!(e.line.chars().count() <= MAP_LINE_MOST && !e.line.contains('\n'));
        assert!(e.line.ends_with('\u{2026}'));
    }

    #[test]
    fn the_addresses_a_text_cites_are_found_once_each_in_order() {
        let text = "See world:area/3#backups, and (story:closed-too-soon). Also WORLD:house. \
                    Again world:area/3#backups. Not this: myworld:x.";
        assert_eq!(
            cited(text),
            [
                "world:area/3#backups",
                "story:closed-too-soon",
                "world:house"
            ]
            .map(str::to_owned)
        );
        assert!(cited("nothing here, not even world: alone").is_empty());
    }
}
