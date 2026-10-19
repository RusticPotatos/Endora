//! The team's map (ADR 0126): the world has a page for every area, project, goal and role, each
//! with a steward worked out from the record; every briefing opens with a short map of the
//! pages that matter to its member; a member who looks and finds nothing says so, apart from the
//! broken roads code finds; and a nightly quiz on the free model tests that the map carries.
//!
//! This module is the rules, as plain data: page addresses, who stewards what, how a map is
//! composed within its size, how an address cited in a page is found, and the shapes of a
//! proposal and a gap.

use super::brain::Brain;
use super::situations::Weight;
use super::work::{GoalId, Member, MemberId, Millis, ProjectId, Role, RunId};
use super::world::{DAY_MS, Snapshot};
use super::{Project, ProjectStatus};

/// The most characters of a gap's `what` and `where_looked`.
pub const GAP_MOST: usize = 200;
/// The most proposals a member may have open at once.
pub const PROPOSALS_OPEN_MOST: usize = 5;
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
        Brain::Sonnet | Brain::Haiku | Brain::Opus => MAP_CHARS,
    }
}
/// The most characters of one line of a map.
pub const MAP_LINE_MOST: usize = 190;
/// The most sections of one page a map points at.
pub const MAP_SECTIONS_PER_PAGE: usize = 3;
/// The line that ends a map in a briefing; the Claude Code worker gives the block back to a
/// turn whose conversation was compacted.
pub const MAP_END: &str = "(End of your map.)";
/// How long a quiz stays open for its member to answer.
pub const QUIZ_OPEN_MS: Millis = 12 * 60 * 60 * 1000;
/// How many nights running a page must fail the quiz to be flagged to its steward.
pub const FAILING_NIGHTS: usize = 3;

/// A page of the world, by the data it is about. Its slug is its address: `world:<slug>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKey {
    /// What the team knows about the house; the CEO's.
    House,
    /// How the lab is built, derived from code.
    Lab,
    /// Who is on the team, derived from the record.
    Team,
    /// A lead's area.
    Area(MemberId),
    /// A project.
    Project(ProjectId),
    /// A goal.
    Goal(GoalId),
    /// How a role works here.
    Role(Role),
}

/// The slug of a role's page: the tester is what the owner reads for the reviewer.
const fn role_slug(role: Role) -> &'static str {
    match role {
        Role::Ceo => "ceo",
        Role::Lead => "lead",
        Role::Developer => "developer",
        Role::Researcher => "researcher",
        Role::Reviewer => "tester",
    }
}

impl PageKey {
    /// Its slug: `house`, `lab`, `area/3`, `project/2`, `goal/1`, `role/developer`.
    #[must_use]
    pub fn slug(self) -> String {
        match self {
            Self::House => "house".to_owned(),
            Self::Lab => "lab".to_owned(),
            Self::Team => "team".to_owned(),
            Self::Area(lead) => format!("area/{}", lead.0),
            Self::Project(p) => format!("project/{}", p.0),
            Self::Goal(g) => format!("goal/{}", g.0),
            Self::Role(role) => format!("role/{}", role_slug(role)),
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
                    "area" => Self::Area(MemberId(number()?)),
                    "project" => Self::Project(ProjectId(number()?)),
                    "goal" => Self::Goal(GoalId(number()?)),
                    "role" => Self::Role(
                        [
                            Role::Ceo,
                            Role::Lead,
                            Role::Developer,
                            Role::Researcher,
                            Role::Reviewer,
                        ]
                        .into_iter()
                        .find(|r| role_slug(*r) == rest)?,
                    ),
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
            Self::Area(_) => "area",
            Self::Project(_) => "project",
            Self::Goal(_) => "goal",
            Self::Role(_) => "role",
        }
    }

    /// Whether HQ derives it from code and the record each time it is read.
    #[must_use]
    pub const fn derived(self) -> bool {
        matches!(self, Self::Lab | Self::Team)
    }

    /// How long an insight or canon on it stays confirmed without a fresh reading: an area's
    /// thirty days, the others' ninety.
    #[must_use]
    pub const fn horizon_days(self) -> i64 {
        match self {
            Self::Area(_) => 30,
            Self::Lab | Self::Team => 0,
            Self::House | Self::Project(_) | Self::Goal(_) | Self::Role(_) => 90,
        }
    }
}

/// Who stewards a page (ADR 0120's RACI): writes it directly, decides what is proposed to it,
/// and answers for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steward {
    /// The CEO.
    Ceo,
    /// A lead.
    Lead(MemberId),
    /// The owner, who is the board.
    Owner,
    /// HQ's code, which derives the page: nobody writes it.
    Hq,
}

impl Steward {
    /// Whether `member` is the steward.
    #[must_use]
    pub fn is(self, member: &Member) -> bool {
        match self {
            Self::Ceo => member.role == Role::Ceo,
            Self::Lead(id) => member.id == id,
            Self::Owner | Self::Hq => false,
        }
    }

    /// By name for a person to read: `Ada`, `Mara, the CEO`, `the owner`, `HQ`.
    #[must_use]
    pub fn said(self, members: &[Member]) -> String {
        match self {
            Self::Ceo => members
                .iter()
                .find(|m| m.role == Role::Ceo)
                .map_or_else(|| "the CEO".to_owned(), |m| format!("{}, the CEO", m.name)),
            Self::Lead(id) => members
                .iter()
                .find(|m| m.id == id)
                .map_or_else(|| "its lead".to_owned(), |m| m.name.clone()),
            Self::Owner => "the owner".to_owned(),
            Self::Hq => "HQ".to_owned(),
        }
    }

    /// The names a person might answer with when asked who it is: for the quiz.
    #[must_use]
    pub fn answers(self, members: &[Member]) -> Vec<String> {
        match self {
            Self::Ceo => {
                let mut all = vec!["ceo".to_owned()];
                all.extend(
                    members
                        .iter()
                        .filter(|m| m.role == Role::Ceo)
                        .map(|m| m.name.to_lowercase()),
                );
                all
            }
            Self::Lead(id) => members
                .iter()
                .filter(|m| m.id == id)
                .map(|m| m.name.to_lowercase())
                .collect(),
            Self::Owner => vec!["owner".to_owned(), "board".to_owned()],
            Self::Hq => vec!["hq".to_owned(), "code".to_owned()],
        }
    }
}

/// Who stewards a page, from the data: the house and every goal the CEO; an area its lead; a
/// project its lead (the CEO when the CEO leads it, or the lead is gone); a role the owner; a
/// derived page nobody. Never by a name written down: move a project's lead and the stewardship
/// moves with it.
#[must_use]
pub fn steward_of(key: PageKey, members: &[Member], projects: &[Project]) -> Steward {
    let lead = |id: MemberId| match members.iter().find(|m| m.id == id) {
        Some(m) if m.role == Role::Lead => Steward::Lead(m.id),
        _ => Steward::Ceo,
    };
    match key {
        PageKey::House | PageKey::Goal(_) => Steward::Ceo,
        PageKey::Area(id) => lead(id),
        PageKey::Project(p) => projects
            .iter()
            .find(|project| project.id == p)
            .map_or(Steward::Ceo, |project| lead(project.lead)),
        PageKey::Role(_) => Steward::Owner,
        PageKey::Lab | PageKey::Team => Steward::Hq,
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
        text.push_str(
            "Looked and it is not here? Say so with report_gap; do not build it again.\n",
        );
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
    /// `world:area/3`, or `world:area/3#backups` for a section.
    pub address: String,
    /// The page's slug.
    pub page: String,
    /// The page's title, or the section's heading.
    pub title: String,
    /// What the page says, as a phrase: `how the lab is built`.
    pub topic: String,
    /// Who keeps it, by name.
    pub steward: String,
    /// What a person might answer to "who keeps it?", lowercase.
    pub answers: Vec<String>,
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
    pub fn page(
        key: PageKey,
        title: &str,
        topic: &str,
        steward: String,
        answers: Vec<String>,
        weight: Weight,
    ) -> Self {
        Self {
            address: format!("world:{}", key.slug()),
            page: key.slug(),
            title: title.to_owned(),
            topic: topic.to_owned(),
            line: format!("{title}: {topic}. Kept by {steward}."),
            steward,
            answers,
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
            answers: Vec::new(),
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

/// What a role's page starts from: a headline and a body in a few words, until the owner writes
/// it fully. Never more than what the role is for, from the roles' own definitions.
#[must_use]
pub const fn role_seed(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Ceo => (
            "Runs the team for the owner, who is the board.",
            "Decides what the leads ask, reviews the house once a day, delegates to leads and \
             never to the weeds, and puts to the board only what the owner alone decides.",
        ),
        Role::Lead => (
            "Plans an area's work, hands its parts on and accepts tested work.",
            "Keeps its area's page, decides stuck work in its area, and accepts or sends back \
             what the tester passed, against the task's criteria.",
        ),
        Role::Developer => (
            "Writes and changes code in the fork, on a branch, and submits it with evidence.",
            "Claims a task, reads before it writes, tests, and submits for review with the \
             readings that show each criterion holds. Pushes only through the gateway.",
        ),
        Role::Researcher => (
            "Finds things out and writes them up on evidence from HQ's record.",
            "Reads, compares and reports; makes no change to the lab.",
        ),
        Role::Reviewer => (
            "Re-checks submitted work with a reading of her own; passes it or sends it back.",
            "Never makes work and never reviews her own. Sending back names exactly what does \
             not hold; a story's handle can stand in for the paragraph.",
        ),
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

/// A road that does not lead where it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Road {
    /// The section that cites it.
    pub from: String,
    /// The address it cites.
    pub to: String,
    /// Why it does not resolve, in words.
    pub why: String,
}

// ---- proposals and gaps -------------------------------------------------------------------

/// Where a proposal to a page stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStatus {
    /// Waiting for the steward.
    Open,
    /// The steward wrote it.
    Accepted,
    /// The steward declined it, with a reason.
    Declined,
}

impl ProposalStatus {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
        }
    }

    /// The status with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Open, Self::Accepted, Self::Declined]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// A change a member proposes to a page it does not steward: a section, new or revised, that the
/// steward writes or declines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageProposal {
    /// Its row; 0 before it is kept.
    pub id: i64,
    /// The page's slug.
    pub page: String,
    /// The section it would revise, by anchor.
    pub anchor: Option<String>,
    /// The section's heading.
    pub heading: String,
    /// What a briefing shows of it.
    pub headline: String,
    /// The rest of it.
    pub body: String,
    /// What it rests on.
    pub evidence: Vec<Snapshot>,
    /// Who proposed it.
    pub by: MemberId,
    /// The turn it was proposed in.
    pub run: Option<RunId>,
    /// When.
    pub at: Millis,
    /// Where it stands.
    pub status: ProposalStatus,
    /// Who decided it: a name, or `the owner`.
    pub decided_by: String,
    /// Why it was declined (or a word on accepting).
    pub why: String,
    /// When it was decided.
    pub decided_at: Option<Millis>,
    /// Whether the proposer has been told how it went.
    pub told: bool,
}

/// A thing a member looked for on the map and did not find. Kept apart from a broken road,
/// which is a road code finds: this is knowledge that is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// Its row; 0 before it is kept.
    pub id: i64,
    /// What was looked for.
    pub what: String,
    /// Where the member looked.
    pub where_looked: String,
    /// What makes two reports the same gap.
    pub key: String,
    /// The steward it goes to; `None` for the CEO.
    pub steward: Option<MemberId>,
    /// Who reported it first.
    pub by: MemberId,
    /// The turn it was first reported in.
    pub run: Option<RunId>,
    /// How many times it was reported.
    pub count: i64,
    /// When it was first reported.
    pub opened_at: Millis,
    /// When it was last reported.
    pub last_at: Millis,
    /// When it was closed, if it was.
    pub closed_at: Option<Millis>,
    /// Who closed it.
    pub closed_by: String,
    /// How: what was written, or why not.
    pub how: String,
}

/// What makes two reports of a gap the same: the first ten words of letters and digits.
#[must_use]
pub fn gap_key(what: &str) -> String {
    what.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(10)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Midnight-free day number of `at`: the quiz's night.
#[must_use]
pub const fn night_of(at: Millis, offset_minutes: i64) -> i64 {
    (at + offset_minutes * 60_000).div_euclid(DAY_MS)
}

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

    fn member(id: i64, name: &str, role: Role) -> Member {
        Member {
            id: MemberId(id),
            name: name.to_owned(),
            role,
            share_percent: 10,
            slots: 2,
            look: 0,
            paused: false,
            brain: Brain::Haiku,
            work_brain: Brain::Mac,
            reports_to: None,
            furloughed_at: None,
            recalled_at: None,
        }
    }

    #[test]
    fn a_page_has_one_slug_and_reads_back() {
        for key in [
            PageKey::House,
            PageKey::Lab,
            PageKey::Team,
            PageKey::Area(MemberId(3)),
            PageKey::Project(ProjectId(2)),
            PageKey::Goal(GoalId(1)),
            PageKey::Role(Role::Developer),
            PageKey::Role(Role::Reviewer),
        ] {
            assert_eq!(PageKey::parse(&key.slug()), Some(key), "{}", key.slug());
        }
        assert_eq!(PageKey::Role(Role::Reviewer).slug(), "role/tester");
        for wrong in [
            "",
            "area",
            "area/x",
            "area/03",
            "role/janitor",
            "garage",
            "house/1",
        ] {
            assert_eq!(PageKey::parse(wrong), None, "{wrong}");
        }
        assert!(PageKey::Lab.derived() && PageKey::Team.derived() && !PageKey::House.derived());
        assert_eq!(PageKey::Area(MemberId(1)).horizon_days(), 30);
        assert_eq!(PageKey::Project(ProjectId(1)).horizon_days(), 90);
    }

    #[test]
    fn a_pages_steward_comes_from_the_data_never_from_a_name() {
        let members = vec![
            member(1, "Mara", Role::Ceo),
            member(2, "Ada", Role::Lead),
            member(3, "Bo", Role::Lead),
            member(4, "Cy", Role::Developer),
        ];
        let project = |id: i64, lead: i64| Project {
            id: ProjectId(id),
            goal: GoalId(1),
            title: "p".to_owned(),
            reason: String::new(),
            lead: MemberId(lead),
            status: ProjectStatus::Active,
            pause: None,
            cap: None,
            asked_cap: None,
            created_by: "owner".to_owned(),
            proposer: None,
            proposed_in: None,
            evidence: Vec::new(),
            created_at: 0,
            updated_at: 0,
        };
        let projects = vec![project(1, 2), project(2, 1), project(3, 9)];
        let of = |key| steward_of(key, &members, &projects);
        assert_eq!(of(PageKey::House), Steward::Ceo);
        assert_eq!(of(PageKey::Goal(GoalId(1))), Steward::Ceo);
        assert_eq!(of(PageKey::Area(MemberId(3))), Steward::Lead(MemberId(3)));
        assert_eq!(of(PageKey::Area(MemberId(4))), Steward::Ceo, "not a lead");
        assert_eq!(
            of(PageKey::Project(ProjectId(1))),
            Steward::Lead(MemberId(2))
        );
        assert_eq!(
            of(PageKey::Project(ProjectId(2))),
            Steward::Ceo,
            "CEO leads"
        );
        assert_eq!(
            of(PageKey::Project(ProjectId(3))),
            Steward::Ceo,
            "its lead is gone"
        );
        assert_eq!(of(PageKey::Role(Role::Developer)), Steward::Owner);
        assert_eq!(of(PageKey::Lab), Steward::Hq);
        assert!(Steward::Ceo.is(&members[0]) && !Steward::Ceo.is(&members[1]));
        assert!(Steward::Lead(MemberId(2)).is(&members[1]));
        assert!(!Steward::Owner.is(&members[0]));
        assert_eq!(Steward::Ceo.said(&members), "Mara, the CEO");
        assert_eq!(Steward::Lead(MemberId(2)).said(&members), "Ada");
        assert_eq!(Steward::Lead(MemberId(2)).answers(&members), ["ada"]);
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
        assert!(whole.text.contains("report_gap"));
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

    #[test]
    fn two_reports_of_one_gap_share_a_key() {
        assert_eq!(
            gap_key("How backups are  verified?"),
            gap_key("how backups are verified")
        );
        assert_ne!(gap_key("how backups run"), gap_key("how restores run"));
        assert_eq!(night_of(1_792_324_800_000, 0), 20_744);
        assert_eq!(night_of(1_792_324_800_000, 12 * 60), 20_745);
    }
}
