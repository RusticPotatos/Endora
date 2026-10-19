//! The team's world (ADR 0122): what the team knows, kept as addressed pieces with a stage, so
//! a member can tell a reading from a hunch and the owner can tell what is settled.
//!
//! - **Pages** hold **sections**, each at a stable address (`world:house#backups`) that a stage
//!   change or a new heading never moves. The house page is written, and stewarded by the CEO;
//!   the lab and team pages are derived from code and the record each time they are read, so
//!   they are never stale. Every page also has a dated, append-only timeline.
//! - **Stages** climb from an observation to canon. Only the owner makes something canon. How far
//!   a piece is trusted is worked out when it is read, from its dated evidence: an insight or
//!   canon not confirmed within its page's horizon, or whose evidence has all left the record,
//!   reads as unconfirmed and counts as an observation.
//! - **Stories** carry a lesson as a moment, the tempting move, what to do instead and why,
//!   under a short handle that a post can cite in its place.
//! - A **handover note** says what is done, what is not, what comes next and what to watch out
//!   for, whenever work is handed on.

use core::fmt;

use super::evidence::{Found, Ref};
use super::work::{MemberId, Millis, RunId};

/// A day, in milliseconds.
pub const DAY_MS: Millis = 24 * 60 * 60 * 1000;

/// The most characters of a section's heading.
pub const HEADING_MOST: usize = 80;
/// The most characters of a section's headline: what a briefing shows of it in one line.
pub const HEADLINE_MOST: usize = 120;
/// The most characters of a section's body.
pub const BODY_MOST: usize = 1_200;
/// The most characters of a warrant: why a piece is believed, in one line.
pub const WARRANT_MOST: usize = 300;
/// The most characters of one line of a page's timeline.
pub const TIMELINE_LINE_MOST: usize = 500;
/// The most characters of each part of a story.
pub const STORY_PART_MOST: usize = 300;
/// The most characters of a whole handover note.
pub const HANDOVER_MOST: usize = 1_200;
/// The most characters of a reason for a stage change.
pub const WHY_MOST: usize = 300;

/// How established a piece of knowledge is, from a single reading to what the house treats as
/// settled; and the two ways a piece stops being current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Something seen once, on at least one piece of evidence.
    Observation,
    /// A member's own note in its notebook (ADR 0118).
    FieldNote,
    /// What a page's steward accepts, on evidence from two turns or a fresh witnessed reading.
    Insight,
    /// What the house treats as established: the owner's word alone.
    Canon,
    /// Replaced by another piece, whose address it names.
    Superseded,
    /// No longer true or needed, with the reason.
    Retired,
}

impl Stage {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::FieldNote => "field_note",
            Self::Insight => "insight",
            Self::Canon => "canon",
            Self::Superseded => "superseded",
            Self::Retired => "retired",
        }
    }

    /// As the owner and the members read it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::FieldNote => "field note",
            other => other.name(),
        }
    }

    /// The stage with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Observation,
            Self::FieldNote,
            Self::Insight,
            Self::Canon,
            Self::Superseded,
            Self::Retired,
        ]
        .into_iter()
        .find(|s| s.name() == name.trim())
    }

    /// Whether the piece is still current: neither superseded nor retired.
    #[must_use]
    pub const fn current(self) -> bool {
        !matches!(self, Self::Superseded | Self::Retired)
    }
}

/// One of the world's pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Its address: `world:<slug>`.
    pub slug: &'static str,
    /// Its title.
    pub title: &'static str,
    /// Whether HQ derives it from code and the record each time it is read.
    pub derived: bool,
    /// How long an insight or canon on it stays confirmed without a fresh reading.
    pub horizon_days: i64,
}

/// The house page: what the team knows about the house, kept by the CEO.
pub const HOUSE: Page = Page {
    slug: "house",
    title: "The house",
    derived: false,
    horizon_days: 90,
};

/// How the lab is built (ADR 0112), from code: never stale.
pub const LAB: Page = Page {
    slug: "lab",
    title: "How the lab is built",
    derived: true,
    horizon_days: 0,
};

/// The team: who is on it, in what role, on what brain, reporting to whom; from the record.
pub const TEAM: Page = Page {
    slug: "team",
    title: "The team",
    derived: true,
    horizon_days: 0,
};

/// Every page the world has.
pub const PAGES: [Page; 3] = [HOUSE, LAB, TEAM];

/// The page at `slug`, if there is one.
#[must_use]
pub fn page(slug: &str) -> Option<Page> {
    PAGES.into_iter().find(|p| p.slug == slug)
}

/// Where a piece of the world is: a page, a section of one, or a story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// A whole page: `world:house`, `world:area/3`.
    Page(String),
    /// One section: `world:house#backups`.
    Section {
        /// Its page's slug.
        page: String,
        /// Its anchor, stable for its life.
        anchor: String,
    },
    /// A story, by its handle: `story:closed-too-soon`.
    Story(String),
}

impl Address {
    /// An address as a member writes it: `world:house#backups`, `house#backups`, `world:lab`,
    /// `world:area/3#backups`, `story:closed-too-soon`, case aside. Whether the page is there
    /// is for the record to say.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        if let Some(handle) = text.strip_prefix("story:") {
            return handle_of(handle).ok().map(Self::Story);
        }
        let text = text.strip_prefix("world:").unwrap_or(&text);
        let (slug, anchor) = match text.split_once('#') {
            Some((slug, anchor)) => (slug, Some(anchor)),
            None => (text, None),
        };
        let page = super::map::PageKey::parse(slug)?.slug();
        let Some(anchor) = anchor else {
            return Some(Self::Page(page));
        };
        let made = anchor_of(anchor)?;
        (made == anchor).then_some(Self::Section { page, anchor: made })
    }

    /// As it is cited: `world:house#backups`, `story:closed-too-soon`.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::Page(page) => format!("world:{page}"),
            Self::Section { page, anchor } => format!("world:{page}#{anchor}"),
            Self::Story(handle) => format!("story:{handle}"),
        }
    }
}

/// The anchor a heading gets: lowercase letters and digits, words joined by `-`, at most 40
/// characters. `None` for a heading with no letter or digit in it.
#[must_use]
pub fn anchor_of(heading: &str) -> Option<String> {
    let mut anchor = String::new();
    for c in heading.trim().to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            anchor.push(c);
        } else if !anchor.is_empty() && !anchor.ends_with('-') {
            anchor.push('-');
        }
    }
    let mut anchor: String = anchor.trim_end_matches('-').chars().take(40).collect();
    while anchor.ends_with('-') {
        anchor.pop();
    }
    (!anchor.is_empty()).then_some(anchor)
}

/// Words kept: on one line, cleaned of what could turn text around or hide in it and of an `@`
/// that would wake a member ([`super::record::clean`]), within `most` characters. What a member
/// writes here is read by the next to open the page (ADR 0129).
///
/// # Errors
/// [`WorldRefusal::TooLong`] past `most`.
pub fn one_line(text: &str, most: usize) -> Result<String, WorldRefusal> {
    let kept = super::record::clean(text);
    if kept.chars().count() > most {
        return Err(WorldRefusal::TooLong(most));
    }
    Ok(kept)
}

/// A section's body kept: Markdown lines, scrubbed of what could turn text around or hide in it
/// ([`super::record::scrub`]: the line breaks stay), and with no page or section heading inside
/// it (`#` or `##`): a section is one piece.
///
/// # Errors
/// [`WorldRefusal::TooLong`]; [`WorldRefusal::HeadingInBody`].
pub fn body(text: &str) -> Result<String, WorldRefusal> {
    let kept = super::record::scrub(text).trim().to_owned();
    if kept.chars().count() > BODY_MOST {
        return Err(WorldRefusal::TooLong(BODY_MOST));
    }
    let heading = |line: &str| {
        let line = line.trim_start();
        line.starts_with("# ") || line.starts_with("## ") || line == "#" || line == "##"
    };
    if kept.lines().any(heading) {
        return Err(WorldRefusal::HeadingInBody);
    }
    Ok(kept)
}

/// A piece of evidence as a page keeps it: the reference and what it said when it was cited,
/// so that a piece still shows its evidence after the feed has let an old event go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The reference.
    pub cited: Ref,
    /// What the record said of it then.
    pub said: String,
}

impl Snapshot {
    /// The list as stored.
    #[must_use]
    pub fn keep(all: &[Self]) -> String {
        serde_json::Value::Array(
            all.iter()
                .map(|s| serde_json::json!({ "ref": s.cited.said(), "said": s.said }))
                .collect(),
        )
        .to_string()
    }

    /// The list as stored, read back; what does not read is left out.
    #[must_use]
    pub fn read(kept: &str) -> Vec<Self> {
        let Ok(serde_json::Value::Array(list)) = serde_json::from_str(kept) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|item| {
                Some(Self {
                    cited: Ref::parse(item["ref"].as_str()?)?,
                    said: item["said"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect()
    }

    /// The references alone.
    #[must_use]
    pub fn refs(all: &[Self]) -> Vec<Ref> {
        all.iter().map(|s| s.cited).collect()
    }
}

/// One section of a written page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Its row.
    pub id: i64,
    /// Its page's slug.
    pub page: String,
    /// Its anchor: set when it is first written, never changed.
    pub anchor: String,
    /// Its heading.
    pub heading: String,
    /// What a briefing shows of it, in one line.
    pub headline: String,
    /// The rest of it, in Markdown.
    pub body: String,
    /// How established it is, as stored.
    pub stage: Stage,
    /// Why it is believed, in one line.
    pub warrant: String,
    /// What it rests on.
    pub evidence: Vec<Snapshot>,
    /// When it was last confirmed on fresh evidence.
    pub verified_at: Option<Millis>,
    /// Who confirmed it.
    pub verified_by: String,
    /// The address of what replaced it, once superseded.
    pub superseded_by: Option<String>,
    /// Why its stage last changed, by whom (a demotion, a retirement).
    pub why: String,
    /// Who wrote its latest revision.
    pub written_by: String,
    /// The turn that wrote its latest revision, if a member's.
    pub written_in_run: Option<RunId>,
    /// Its revision: 1 when first written.
    pub revision: i64,
    /// When its latest revision was written.
    pub updated_at: Millis,
    /// When a post last cited it; its writing, until then.
    pub last_used_at: Millis,
}

impl Section {
    /// Its address.
    #[must_use]
    pub fn address(&self) -> String {
        format!("world:{}#{}", self.page, self.anchor)
    }
}

/// How far a piece is trusted now, worked out when it is read and never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trust {
    /// The stage it counts as: its own, or an observation once unconfirmed.
    pub stage: Stage,
    /// Since when it has gone unconfirmed, if it has.
    pub unconfirmed_since: Option<Millis>,
}

/// How far a piece at `stage` is trusted at `now`: an insight or canon whose last confirmation
/// (`confirmed`) is older than `horizon_days`, or none of whose evidence is still in the record
/// (`resolves` false), counts as an observation, unconfirmed since that confirmation. Every
/// other stage is taken as it is.
#[must_use]
pub fn trust(
    stage: Stage,
    confirmed: Millis,
    horizon_days: i64,
    now: Millis,
    resolves: bool,
) -> Trust {
    let trusted = Trust {
        stage,
        unconfirmed_since: None,
    };
    if !matches!(stage, Stage::Insight | Stage::Canon) {
        return trusted;
    }
    if resolves && now - confirmed <= horizon_days * DAY_MS {
        return trusted;
    }
    Trust {
        stage: Stage::Observation,
        unconfirmed_since: Some(confirmed),
    }
}

/// What a writer's evidence must be for the stage it asks: an observation rests on at least one
/// reference in the record; an insight also on a one-line warrant, and on references from at
/// least two different turns (`turns_behind` counts them) or a reading the node witnessed in
/// this turn (`this_turn`). Canon is never written: the owner makes it.
///
/// # Errors
/// [`WorldRefusal::NeedsEvidence`]; [`super::work::Refusal::NoSuchEvidence`] naming the first
/// missing reference; [`WorldRefusal::NeedsWarrant`]; [`WorldRefusal::NotAnInsight`];
/// [`WorldRefusal::CanonIsTheOwners`]; [`WorldRefusal::NotAWritableStage`].
pub fn warranted(
    stage: Stage,
    found: &[(Ref, Found)],
    turns_behind: usize,
    warrant: &str,
    this_turn: RunId,
) -> Result<(), super::work::Refusal> {
    use super::work::Refusal;
    match stage {
        Stage::Canon => return Err(Refusal::World(WorldRefusal::CanonIsTheOwners)),
        Stage::Observation | Stage::Insight => {}
        other => {
            return Err(Refusal::World(WorldRefusal::NotAWritableStage(
                other.name().to_owned(),
            )));
        }
    }
    if found.is_empty() {
        return Err(Refusal::World(WorldRefusal::NeedsEvidence));
    }
    if let Some((missing, _)) = found.iter().find(|(_, f)| *f == Found::Missing) {
        return Err(Refusal::NoSuchEvidence(missing.said()));
    }
    if stage == Stage::Observation {
        return Ok(());
    }
    if warrant.trim().is_empty() {
        return Err(Refusal::World(WorldRefusal::NeedsWarrant));
    }
    let witnessed_now = found.iter().any(|(_, f)| {
        matches!(f, Found::Receipt { run, ok: true, witnessed: true, .. } if *run == this_turn)
    });
    if witnessed_now || (found.len() >= 2 && turns_behind >= 2) {
        return Ok(());
    }
    Err(Refusal::World(WorldRefusal::NotAnInsight))
}

/// Whom a story belongs to: the house, a lead's area, or one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The whole house: the CEO's to keep.
    House,
    /// A lead's area: the lead, and whoever reports up to it (ADR 0120).
    Area(MemberId),
    /// One member's own: it follows the member across brains.
    Member(MemberId),
}

impl Scope {
    /// Its stable name: `house`, `area:3`, `member:5`.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::House => "house".to_owned(),
            Self::Area(lead) => format!("area:{}", lead.0),
            Self::Member(member) => format!("member:{}", member.0),
        }
    }

    /// The scope with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if name == "house" {
            return Some(Self::House);
        }
        let (kind, id) = name.split_once(':')?;
        let id = MemberId(id.parse().ok()?);
        match kind {
            "area" => Some(Self::Area(id)),
            "member" => Some(Self::Member(id)),
            _ => None,
        }
    }

    /// How long a story in it stays confirmed without a fresh reading: the house's like the
    /// house page's, an area's and a member's like the lab's.
    #[must_use]
    pub const fn horizon_days(self) -> i64 {
        match self {
            Self::House => HOUSE.horizon_days,
            Self::Area(_) | Self::Member(_) => 30,
        }
    }
}

/// A story's handle as written: two to four words of lowercase letters and digits joined by
/// `-` (`closed-too-soon`, `restart-is-not-a-fix`), at most 48 characters.
///
/// # Errors
/// [`WorldRefusal::NotAHandle`].
pub fn handle_of(text: &str) -> Result<String, WorldRefusal> {
    let made: String = text
        .trim()
        .to_ascii_lowercase()
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let parts: Vec<&str> = made.split('-').filter(|w| !w.is_empty()).collect();
    let words = parts.iter().all(|w| {
        w.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    if !(2..=4).contains(&parts.len()) || !words || made.len() > 48 {
        return Err(WorldRefusal::NotAHandle(
            text.trim().chars().take(60).collect(),
        ));
    }
    Ok(made)
}

/// A handle made from a lesson's first words, apart from every one `taken`: its first three
/// words of letters and digits (`lesson` after a lone one), then `-2`, `-3` until it is free.
#[must_use]
pub fn handle_from(words: &str, taken: &[String]) -> String {
    let mut parts: Vec<String> = words
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .take(3)
        .collect();
    while parts.len() < 2 {
        parts.push("lesson".to_owned());
    }
    let first = parts.join("-");
    let mut made = first.clone();
    let mut n = 2;
    while taken.contains(&made) {
        made = format!("{first}-{n}");
        n += 1;
    }
    made
}

/// One story: a lesson in four parts under a handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    /// Its row.
    pub id: i64,
    /// Whose it is.
    pub scope: Scope,
    /// Its handle: `story:<handle>` is its address.
    pub handle: String,
    /// When it matters.
    pub moment: String,
    /// The tempting wrong move.
    pub reflex: String,
    /// What to do instead.
    pub instead: String,
    /// Why: the failure or the evidence behind it.
    pub because: String,
    /// What it rests on.
    pub evidence: Vec<Snapshot>,
    /// How established it is, as stored.
    pub stage: Stage,
    /// Why it is believed, for an insight or canon.
    pub warrant: String,
    /// The situations in which a briefing tells it whole; otherwise only its handle and what to
    /// do instead.
    pub taught: Vec<super::situations::Situation>,
    /// Who told it.
    pub author: String,
    /// The turn it was told in, if a member's.
    pub written_in_run: Option<RunId>,
    /// When it was told (for a migrated lesson, when the lesson was noted).
    pub written_at: Millis,
    /// Whether it is a one-line lesson from before stories, still to be retold whole.
    pub to_retell: bool,
    /// The lesson's line in a goal's notes it was made from, if it was.
    pub migrated_from: Option<String>,
    /// When it was last confirmed on fresh evidence.
    pub verified_at: Option<Millis>,
    /// When a post last cited its handle; its telling, until then.
    pub last_used_at: Millis,
    /// Why its stage last changed, by whom.
    pub why: String,
}

impl Story {
    /// Its address.
    #[must_use]
    pub fn address(&self) -> String {
        format!("story:{}", self.handle)
    }

    /// The story told whole, on one line.
    #[must_use]
    pub fn told(&self) -> String {
        let mut parts = Vec::new();
        for (label, part) in [
            ("When", &self.moment),
            ("Tempting", &self.reflex),
            ("Instead", &self.instead),
            ("Because", &self.because),
        ] {
            if !part.is_empty() {
                parts.push(format!("{label}: {part}"));
            }
        }
        format!("{}: {}", self.handle, parts.join(" "))
    }

    /// The story in one short line: its handle and what to do instead, marked when it is still
    /// to be retold.
    #[must_use]
    pub fn short(&self) -> String {
        let retell = if self.to_retell { " (to retell)" } else { "" };
        format!("{}: {}{retell}", self.handle, self.instead)
    }

    /// Whether `text` names the story by its handle, as a word of its own.
    #[must_use]
    pub fn cited_in(&self, text: &str) -> bool {
        text.to_ascii_lowercase()
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .any(|word| word.trim_matches('-') == self.handle)
    }
}

/// What a member says when it hands work on: what is done, what is not, what comes next and
/// what to watch out for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverNote {
    /// What is done.
    pub done: String,
    /// What is not.
    pub not_done: String,
    /// What comes next.
    pub next: String,
    /// What to watch out for.
    pub watch_out: String,
}

impl HandoverNote {
    /// A note, each part said on one line and all of it within [`HANDOVER_MOST`] characters.
    ///
    /// # Errors
    /// [`WorldRefusal::HandoverNeeded`] for a part left empty; [`WorldRefusal::TooLong`].
    pub fn new(
        done: &str,
        not_done: &str,
        next: &str,
        watch_out: &str,
    ) -> Result<Self, WorldRefusal> {
        let parts = [done, not_done, next, watch_out]
            .map(|p| one_line(p, HANDOVER_MOST))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        if parts.iter().any(String::is_empty) {
            return Err(WorldRefusal::HandoverNeeded);
        }
        if parts.iter().map(|p| p.chars().count()).sum::<usize>() > HANDOVER_MOST {
            return Err(WorldRefusal::TooLong(HANDOVER_MOST));
        }
        let [done, not_done, next, watch_out]: [String; 4] =
            parts.try_into().map_err(|_| WorldRefusal::HandoverNeeded)?;
        Ok(Self {
            done,
            not_done,
            next,
            watch_out,
        })
    }

    /// The note on one line.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "Done: {} Not done: {} Next: {} Watch out: {}",
            self.done, self.not_done, self.next, self.watch_out
        )
    }
}

/// A handover note as kept: on which task, from whom to whom, in which turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// Its row.
    pub id: i64,
    /// The task handed on.
    pub task: super::work::TaskId,
    /// Who handed it on: the member whose turn it was.
    pub by: MemberId,
    /// Who held it, if anyone.
    pub from: Option<MemberId>,
    /// Whom it went to; `None` when the work was dropped.
    pub to: Option<MemberId>,
    /// The turn it was handed on in.
    pub run: Option<RunId>,
    /// What was said.
    pub note: HandoverNote,
    /// What it cites, if anything.
    pub evidence: Vec<Ref>,
    /// When.
    pub at: Millis,
}

/// What the world refuses, said so a member can do something about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldRefusal {
    /// Not an address the world has.
    NotAnAddress(String),
    /// Nothing is at that address.
    Nothing(String),
    /// A page or story that is someone else's to write.
    NotYours {
        /// What it is.
        what: String,
        /// Whose it is.
        steward: String,
    },
    /// No evidence was cited.
    NeedsEvidence,
    /// An insight without a warrant.
    NeedsWarrant,
    /// An insight on too little evidence.
    NotAnInsight,
    /// Only the owner makes something canon, or changes what is.
    CanonIsTheOwners,
    /// A stage nobody writes directly.
    NotAWritableStage(String),
    /// A section with no heading.
    NeedsHeading,
    /// A section's body with a heading of its own inside.
    HeadingInBody,
    /// Longer than it may be.
    TooLong(usize),
    /// Not a handle a story can have.
    NotAHandle(String),
    /// A handle another story already has.
    HandleTaken(String),
    /// A story with a part missing.
    StoryNeedsParts,
    /// Retiring, or demoting, needs the reason.
    NeedsWhy,
    /// Superseding names what replaces it.
    NeedsSuccessor,
    /// Work handed on with no handover note.
    HandoverNeeded,
    /// The house log is the house page's timeline now.
    HouseLogRetired,
    /// Only the CEO and the leads propose canon.
    NotAProposer,
    /// Only a trusted insight is proposed for canon.
    NotReadyForCanon(String),
    /// Something that is not canon cannot be demoted from it; something canon is already.
    NotCanon(String),
    /// Already canon.
    AlreadyCanon(String),
    /// A derived page is HQ's to write.
    Derived(String),
    /// A page that is someone else's to write: propose the change instead (ADR 0126).
    ProposeInstead {
        /// The page.
        what: String,
        /// Whose it is.
        steward: String,
    },
    /// A proposal only the page's steward decides.
    DecidedBy {
        /// The page.
        what: String,
        /// Whose it is.
        steward: String,
    },
    /// A proposal to a page the proposer stewards: it writes it.
    YourOwnPage(String),
    /// No page of the world is called that.
    NoSuchPage(String),
    /// No proposal has that number.
    NoSuchProposal(i64),
    /// The proposal was decided already.
    ProposalDecided(i64),
    /// A member has this many proposals open: they wait for their stewards first.
    TooManyProposals(usize),
    /// A gap says what was looked for and where.
    GapNeedsParts,
    /// No gap has that number.
    NoSuchGap(i64),
    /// The gap is closed already.
    GapClosed(i64),
    /// Closing a gap says how it was answered.
    GapNeedsHow,
    /// The gap is another steward's.
    GapNotYours(i64),
    /// There is no quiz open for this member.
    NoQuizForYou,
    /// A quiz is answered with one answer per question.
    QuizAnswers(usize),
}

impl fmt::Display for WorldRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAddress(said) => write!(
                f,
                "`{said}` is not an address in the world: world:house, world:house#<section>, \
                 world:lab, world:team, world:area/<n>, world:project/<n>, world:goal/<n>, \
                 world:role/<role> (any of them with #<section>) or story:<handle>"
            ),
            Self::Nothing(address) => write!(f, "nothing is at {address}"),
            Self::NotYours { what, steward } => write!(
                f,
                "{what} is {steward}'s to write: tell them what you found, with its evidence, \
                 in a post"
            ),
            Self::NeedsEvidence => f.write_str(
                "it rests on evidence from HQ's record: cite at least one receipt, task or event",
            ),
            Self::NeedsWarrant => {
                f.write_str("an insight says why it is believed, in one line: give a warrant")
            }
            Self::NotAnInsight => f.write_str(
                "an insight rests on evidence from at least two different turns, or on a reading \
                 the node witnessed in this turn: write it as an observation, or cite more",
            ),
            Self::CanonIsTheOwners => f.write_str(
                "only the owner makes something canon, or changes it once it is: propose it with \
                 propose_canon",
            ),
            Self::NotAWritableStage(stage) => write!(
                f,
                "`{stage}` is not written directly: write an observation or an insight (retire \
                 or supersede with retire_world)"
            ),
            Self::NeedsHeading => f.write_str("a section has a heading"),
            Self::HeadingInBody => f.write_str(
                "a section is one piece: no `#` or `##` heading inside its body (`###` is fine)",
            ),
            Self::TooLong(most) => write!(f, "at most {most} characters"),
            Self::NotAHandle(said) => write!(
                f,
                "`{said}` is not a handle: two or three short words, like closed-too-soon"
            ),
            Self::HandleTaken(handle) => write!(
                f,
                "another story is already called {handle}: give this one its own handle"
            ),
            Self::StoryNeedsParts => f.write_str(
                "a story says when it matters (moment), the tempting move (reflex), what to do \
                 instead (instead) and why (because)",
            ),
            Self::NeedsWhy => f.write_str("say why"),
            Self::NeedsSuccessor => {
                f.write_str("superseding names the address of what replaces it")
            }
            Self::HandoverNeeded => f.write_str(
                "handing work on needs a handover note: handover { done, not_done, next, \
                 watch_out }, each said",
            ),
            Self::HouseLogRetired => f.write_str(
                "the house log is now the house page (world:house): note_house adds a dated line \
                 to its timeline, and write_world writes a section of it",
            ),
            Self::NotAProposer => f.write_str("only the CEO and the leads propose canon"),
            Self::NotReadyForCanon(address) => write!(
                f,
                "{address} is not a trusted insight: confirm it (verify_world) before proposing it"
            ),
            Self::NotCanon(address) => write!(f, "{address} is not canon"),
            Self::AlreadyCanon(address) => write!(f, "{address} is canon already"),
            Self::Derived(address) => {
                write!(
                    f,
                    "{address} is derived by HQ from the record: nobody writes it"
                )
            }
            Self::ProposeInstead { what, steward } => write!(
                f,
                "{what} is kept by {steward}: propose the change with propose_world, and \
                 they decide"
            ),
            Self::DecidedBy { what, steward } => write!(
                f,
                "{what} is kept by {steward}: only they decide what is proposed to it"
            ),
            Self::YourOwnPage(page) => write!(
                f,
                "world:{page} is yours: write it with write_world, a proposal is for a page \
                 someone else keeps"
            ),
            Self::NoSuchPage(said) => write!(f, "there is no page {said} in the world"),
            Self::NoSuchProposal(id) => write!(f, "there is no proposal {id}"),
            Self::ProposalDecided(id) => write!(f, "proposal {id} is decided already"),
            Self::TooManyProposals(most) => write!(
                f,
                "you have {most} proposals open: wait for their stewards to decide them"
            ),
            Self::GapNeedsParts => f.write_str(
                "a gap says what you looked for (what) and where you looked (where_looked)",
            ),
            Self::NoSuchGap(id) => write!(f, "there is no gap {id}"),
            Self::GapClosed(id) => write!(f, "gap {id} is closed already"),
            Self::GapNeedsHow => f.write_str(
                "say how the gap was answered (how): the address you wrote, or why nothing",
            ),
            Self::GapNotYours(id) => write!(f, "gap {id} was reported to another steward"),
            Self::NoQuizForYou => f.write_str("there is no quiz open for you"),
            Self::QuizAnswers(n) => {
                write!(f, "answer all {n} questions, one answer each, in order")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::work::{Refusal, TaskId};

    #[test]
    fn an_address_reads_as_members_write_it_and_says_itself_back() {
        for (written, said) in [
            ("world:house#backups", "world:house#backups"),
            ("House#Backups", "world:house#backups"),
            ("world:lab", "world:lab"),
            ("team", "world:team"),
            ("story:Closed-Too-Soon", "story:closed-too-soon"),
            ("World:Area/3#Backups", "world:area/3#backups"),
            ("role/tester", "world:role/tester"),
            ("project/2", "world:project/2"),
        ] {
            assert_eq!(
                Address::parse(written).map(|a| a.said()),
                Some(said.to_owned()),
                "{written}"
            );
        }
        for wrong in [
            "world:garage",
            "house#",
            "house#not an anchor",
            "story:x",
            "",
            "world:area/x",
            "world:role/janitor#a",
        ] {
            assert_eq!(Address::parse(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn an_anchor_is_made_once_from_the_heading() {
        assert_eq!(
            anchor_of("Backups: when they run"),
            Some("backups-when-they-run".to_owned())
        );
        assert_eq!(anchor_of("  ---  "), None);
        assert_eq!(anchor_of(&"a".repeat(60)).map(|a| a.len()), Some(40));
    }

    #[test]
    fn a_body_is_one_piece_within_its_limit() {
        assert_eq!(
            body(" line one\n- two \n"),
            Ok("line one\n- two".to_owned())
        );
        assert_eq!(body("### a sub-part\ntext").map(|b| b.len()), Ok(19));
        assert_eq!(body("## Another"), Err(WorldRefusal::HeadingInBody));
        assert_eq!(body("# Title"), Err(WorldRefusal::HeadingInBody));
        assert_eq!(
            body(&"x".repeat(BODY_MOST + 1)),
            Err(WorldRefusal::TooLong(BODY_MOST))
        );
    }

    #[test]
    fn trust_is_worked_out_at_read_time_from_dated_evidence() {
        let now = 100 * DAY_MS;
        // Fresh: trusted as stored.
        assert_eq!(
            trust(Stage::Insight, now - 30 * DAY_MS, 30, now, true),
            Trust {
                stage: Stage::Insight,
                unconfirmed_since: None
            }
        );
        // A day past its horizon, though nobody edited it: an observation, unconfirmed since.
        let old = now - 31 * DAY_MS;
        assert_eq!(
            trust(Stage::Insight, old, 30, now, true),
            Trust {
                stage: Stage::Observation,
                unconfirmed_since: Some(old)
            }
        );
        assert_eq!(
            trust(Stage::Canon, old, 30, now, true).unconfirmed_since,
            Some(old)
        );
        // Its evidence has all left the record.
        assert_eq!(
            trust(Stage::Canon, now, 90, now, false).stage,
            Stage::Observation
        );
        // An observation is only ever an observation; a retired piece stays retired.
        assert_eq!(
            trust(Stage::Observation, 0, 30, now, false).unconfirmed_since,
            None
        );
        assert_eq!(
            trust(Stage::Retired, 0, 30, now, true).stage,
            Stage::Retired
        );
    }

    #[test]
    fn an_insight_rests_on_two_turns_or_a_fresh_witnessed_reading_and_canon_is_never_written() {
        let me = RunId(9);
        let receipt = |run: i64, witnessed: bool| Found::Receipt {
            run: RunId(run),
            ok: true,
            in_work: true,
            witnessed,
        };
        let task = (Ref::Task(TaskId(3)), Found::Other { in_work: true });
        let r1 = (Ref::Receipt(1), receipt(4, true));
        let r2 = (Ref::Receipt(2), receipt(5, true));
        let mine = (Ref::Receipt(3), receipt(9, true));
        let sandboxed = (Ref::Receipt(4), receipt(9, false));
        assert_eq!(
            warranted(Stage::Observation, &[task], 0, "", me),
            Ok(()),
            "an observation needs one reference"
        );
        assert_eq!(
            warranted(Stage::Observation, &[], 0, "", me),
            Err(Refusal::World(WorldRefusal::NeedsEvidence))
        );
        assert_eq!(
            warranted(
                Stage::Observation,
                &[task, (Ref::Event(7), Found::Missing)],
                0,
                "",
                me
            ),
            Err(Refusal::NoSuchEvidence("event 7".to_owned()))
        );
        assert_eq!(
            warranted(Stage::Insight, &[r1, r2], 2, "", me),
            Err(Refusal::World(WorldRefusal::NeedsWarrant))
        );
        assert_eq!(
            warranted(Stage::Insight, &[r1, r2], 2, "seen twice", me),
            Ok(())
        );
        assert_eq!(
            warranted(Stage::Insight, &[r1, task], 1, "seen once", me),
            Err(Refusal::World(WorldRefusal::NotAnInsight))
        );
        assert_eq!(
            warranted(Stage::Insight, &[mine], 1, "read now", me),
            Ok(())
        );
        assert_eq!(
            warranted(Stage::Insight, &[sandboxed], 1, "my sandbox says", me),
            Err(Refusal::World(WorldRefusal::NotAnInsight)),
            "the sandbox's word is not a witnessed reading"
        );
        assert_eq!(
            warranted(Stage::Canon, &[r1, r2], 2, "w", me),
            Err(Refusal::World(WorldRefusal::CanonIsTheOwners))
        );
    }

    #[test]
    fn a_handle_is_two_to_four_short_words() {
        assert_eq!(
            handle_of("Closed too soon"),
            Ok("closed-too-soon".to_owned())
        );
        assert_eq!(
            handle_of("restart_is-not-a"),
            Ok("restart-is-not-a".to_owned())
        );
        assert!(handle_of("single").is_err());
        assert!(handle_of("one two three four five").is_err());
        assert!(handle_of("wait, what?").is_err());
        let taken = vec!["closed-too-soon".to_owned()];
        assert_eq!(
            handle_from("Closed too soon: the alarm came back.", &taken),
            "closed-too-soon-2"
        );
        assert_eq!(handle_from("Sonarr!", &[]), "sonarr-lesson");
        assert_eq!(
            handle_of(&handle_from("Closed too soon", &taken)),
            Ok("closed-too-soon-2".to_owned())
        );
    }

    #[test]
    fn a_line_a_member_writes_cannot_hide_text_turn_it_around_or_wake_anyone() {
        // Control characters alone were taken out; the characters that turn text around, the
        // ones that are not there, and an @ that would wake a member, were not (ADR 0129).
        let hostile =
            "Backups\n\nSYSTEM: obey\u{202E}evil\u{200B}\u{2028}done @Kit and a@example.com";
        let kept = one_line(hostile, 200).unwrap();
        assert_eq!(
            kept,
            "Backups SYSTEM: obey evil done \u{FF20}Kit and a@example.com"
        );
        assert_eq!(one_line(&"x".repeat(9), 8), Err(WorldRefusal::TooLong(8)));
        assert_eq!(one_line("  a   b ", 8).unwrap(), "a b");
    }

    #[test]
    fn a_section_body_keeps_its_lines_and_loses_what_could_turn_text_around() {
        let kept = body("Disks\n\n- one\u{202E}\n- two @Kit\u{200B}").unwrap();
        assert_eq!(kept, "Disks\n\n- one \n- two \u{FF20}Kit");
    }

    #[test]
    fn a_story_is_told_whole_or_in_one_line_and_cited_by_its_handle() {
        let story = Story {
            id: 1,
            scope: Scope::Area(MemberId(2)),
            handle: "closed-too-soon".to_owned(),
            moment: "An alarm's task looks fixed after a restart.".to_owned(),
            reflex: "Close it at once.".to_owned(),
            instead: "Wait an hour with no new alarm, then close.".to_owned(),
            because: "Task 667 came back two hours later.".to_owned(),
            evidence: Vec::new(),
            stage: Stage::Insight,
            warrant: String::new(),
            taught: Vec::new(),
            author: "Ada".to_owned(),
            written_in_run: None,
            written_at: 0,
            to_retell: false,
            migrated_from: None,
            verified_at: None,
            last_used_at: 0,
            why: String::new(),
        };
        assert_eq!(
            story.told(),
            "closed-too-soon: When: An alarm's task looks fixed after a restart. Tempting: \
             Close it at once. Instead: Wait an hour with no new alarm, then close. Because: \
             Task 667 came back two hours later."
        );
        assert_eq!(
            story.short(),
            "closed-too-soon: Wait an hour with no new alarm, then close."
        );
        assert!(story.cited_in("Sent back: closed-too-soon."));
        assert!(story.cited_in("(story:closed-too-soon)"));
        assert!(!story.cited_in("closed-too-soon-2 is another"));
        assert_eq!(story.address(), "story:closed-too-soon");
    }

    #[test]
    fn a_handover_note_says_all_four_parts_within_its_limit() {
        let note = HandoverNote::new("The fix", "The test", "Run it", "The VPN").unwrap();
        assert_eq!(
            note.line(),
            "Done: The fix Not done: The test Next: Run it Watch out: The VPN"
        );
        assert_eq!(
            HandoverNote::new("The fix", " ", "Run it", "The VPN"),
            Err(WorldRefusal::HandoverNeeded)
        );
        let long = "x".repeat(400);
        assert_eq!(
            HandoverNote::new(&long, &long, &long, "y"),
            Err(WorldRefusal::TooLong(HANDOVER_MOST))
        );
    }

    #[test]
    fn scopes_and_stages_keep_their_names() {
        for scope in [
            Scope::House,
            Scope::Area(MemberId(3)),
            Scope::Member(MemberId(5)),
        ] {
            assert_eq!(Scope::from_name(&scope.name()), Some(scope));
        }
        assert_eq!(Scope::from_name("garage:1"), None);
        assert_eq!(Scope::Area(MemberId(1)).horizon_days(), 30);
        assert_eq!(Scope::House.horizon_days(), 90);
        for stage in [
            Stage::Observation,
            Stage::FieldNote,
            Stage::Insight,
            Stage::Canon,
            Stage::Superseded,
            Stage::Retired,
        ] {
            assert_eq!(Stage::from_name(stage.name()), Some(stage));
        }
        assert_eq!(Stage::FieldNote.said(), "field note");
        assert!(!Stage::Retired.current() && Stage::Canon.current());
    }

    #[test]
    fn evidence_snapshots_keep_what_the_record_said() {
        let kept = vec![Snapshot {
            cited: Ref::Event(340),
            said: "alarm: deluge unhealthy".to_owned(),
        }];
        assert_eq!(Snapshot::read(&Snapshot::keep(&kept)), kept);
        assert_eq!(Snapshot::read("not json"), Vec::new());
        assert_eq!(Snapshot::refs(&kept), vec![Ref::Event(340)]);
    }
}
