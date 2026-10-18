//! Gaps and broken roads (ADR 0126): the two ways the map fails, kept apart because they are
//! mended differently. A **gap** is knowledge that is missing: a member looked on its map, found
//! nothing and said so; it goes to its lead as one item, never to the owner's desk. A **broken
//! road** is a way that leads nowhere: a section cites an address that does not resolve, found by
//! code each time it is looked for, and shown to the steward of the page that cites it.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::map::{self, GAP_MOST, Gap, PageKey, Road, Steward};
use crate::domain::world::{self, Address, Stage, WorldRefusal};
use crate::domain::{EventKind, Member, Refusal, Role, RunId, staffing};

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

/// The most items a briefing lists before it says how many more there are.
const LISTED: usize = 5;
/// The most hops of a chain of superseded sections followed before giving up.
const HOPS: usize = 5;

/// What reporting a gap came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapReported {
    /// The gap's number.
    pub id: i64,
    /// Whom it went to, by name.
    pub steward: String,
    /// Whether the same gap was reported before: it is one item, and now has one more report.
    pub already: bool,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- gaps ------------------------------------------------------------------------------

    /// A turn says it looked on its map for something and did not find it. The gap goes to the
    /// member's lead (to the CEO for a lead, or a member with no lead), once however many report
    /// it; the owner's desk is not troubled.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::GapNeedsParts`] for no `what` or no `where_looked`;
    /// [`WorldRefusal::TooLong`].
    pub fn report_gap_in(&self, run: RunId, what: &str, where_looked: &str) -> Done<GapReported> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let what = world::one_line(what, GAP_MOST).map_err(refused)?;
        let where_looked = world::one_line(where_looked, GAP_MOST).map_err(refused)?;
        if what.is_empty() || where_looked.is_empty() {
            return Err(refused(WorldRefusal::GapNeedsParts));
        }
        let members = self.store.members()?;
        let steward = match staffing::lead_of(&who, &members) {
            Some(lead) if lead != who.id => Some(lead),
            _ => None,
        };
        let named = match steward {
            Some(lead) => Steward::Lead(lead),
            None => Steward::Ceo,
        }
        .said(&members);
        let key = map::gap_key(&what);
        let now = self.clock.now();
        let same = self
            .store
            .gaps()?
            .into_iter()
            .find(|g| g.closed_at.is_none() && g.key == key && g.steward == steward);
        if let Some(mut gap) = same {
            gap.count += 1;
            gap.last_at = now;
            self.store.save_gap(&gap)?;
            return Ok(GapReported {
                id: gap.id,
                steward: named,
                already: true,
            });
        }
        let id = self.store.save_gap(&Gap {
            id: 0,
            what: what.clone(),
            where_looked,
            key,
            steward,
            by: who.id,
            run: Some(turn.id),
            count: 1,
            opened_at: now,
            last_at: now,
            closed_at: None,
            closed_by: String::new(),
            how: String::new(),
        })?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "gap reported", "gap": id, "by": who.name, "looked_for": what,
                    "to": named }),
        )?;
        if let Some(task) = turn.task {
            self.mark_progress(who.id, task)?;
        }
        Ok(GapReported {
            id,
            steward: named,
            already: false,
        })
    }

    /// Whether `who` is the steward a gap went to.
    fn gap_is_for(gap: &Gap, who: &Member) -> bool {
        match gap.steward {
            Some(lead) => lead == who.id,
            None => who.role == Role::Ceo,
        }
    }

    /// The steward's turn closes a gap, saying how it was answered.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::NoSuchGap`]; [`WorldRefusal::GapClosed`];
    /// [`WorldRefusal::GapNotYours`]; [`WorldRefusal::GapNeedsHow`].
    pub fn close_gap_in(&self, run: RunId, id: i64, how: &str) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let mut gap = self
            .store
            .gaps()?
            .into_iter()
            .find(|g| g.id == id)
            .ok_or_else(|| refused(WorldRefusal::NoSuchGap(id)))?;
        if gap.closed_at.is_some() {
            return Err(refused(WorldRefusal::GapClosed(id)));
        }
        if !Self::gap_is_for(&gap, &who) {
            return Err(refused(WorldRefusal::GapNotYours(id)));
        }
        let how = world::one_line(how, GAP_MOST).map_err(refused)?;
        if how.is_empty() {
            return Err(refused(WorldRefusal::GapNeedsHow));
        }
        gap.closed_at = Some(self.clock.now());
        gap.closed_by = who.name.clone();
        gap.how.clone_from(&how);
        self.store.save_gap(&gap)?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "gap closed", "gap": id, "by": who.name, "how": how }),
        )
    }

    /// The gaps reported to `member`, still open, for its briefing; empty when there are none.
    pub(super) fn gaps_to_answer(&self, member: &Member) -> Done<String> {
        let mine: Vec<Gap> = self
            .store
            .gaps()?
            .into_iter()
            .filter(|g| g.closed_at.is_none() && Self::gap_is_for(g, member))
            .collect();
        if mine.is_empty() {
            return Ok(String::new());
        }
        let mut said = String::from(
            "Gaps reported to you (members looked on their maps and found nothing; answer one by \
             writing the page or section, then close_gap {id, how}):\n",
        );
        for g in mine.iter().take(LISTED) {
            let by = self.member(g.by)?.name;
            let more = if g.count > 1 {
                format!(", and {} more times", g.count - 1)
            } else {
                String::new()
            };
            said.push_str(&format!(
                "- #{} \"{}\" \u{2014} looked in: {}; reported by {by}{more}.\n",
                g.id, g.what, g.where_looked
            ));
        }
        if mine.len() > LISTED {
            said.push_str(&format!("- and {} more.\n", mine.len() - LISTED));
        }
        Ok(said)
    }

    // ---- broken roads ----------------------------------------------------------------------

    /// Why `address` does not resolve, or `None` when it does: the page or section is there and
    /// current, or it was superseded by something that is.
    fn unresolved(&self, address: &str) -> Done<Option<String>> {
        let mut at = address.to_owned();
        for _ in 0..HOPS {
            let Some(parsed) = Address::parse(&at) else {
                return Ok(Some("it is not an address in the world".to_owned()));
            };
            match parsed {
                Address::Page(slug) => {
                    let there = PageKey::parse(&slug)
                        .map(|k| self.page_exists(k))
                        .transpose()?;
                    return Ok((there != Some(true)).then(|| "there is no such page".to_owned()));
                }
                Address::Story(handle) => {
                    return Ok(match self.story_called(&handle)? {
                        None => Some("there is no such story".to_owned()),
                        Some(s) if s.stage == Stage::Retired => {
                            Some(format!("the story is retired ({})", s.why))
                        }
                        Some(_) => None,
                    });
                }
                Address::Section { page, anchor } => {
                    let there = PageKey::parse(&page)
                        .map(|k| self.page_exists(k))
                        .transpose()?;
                    if there != Some(true) {
                        return Ok(Some("there is no such page".to_owned()));
                    }
                    let Some(s) = self.section_at(&page, &anchor)? else {
                        return Ok(Some("nothing is written there".to_owned()));
                    };
                    match (s.stage, s.superseded_by) {
                        (Stage::Retired, _) => {
                            return Ok(Some(format!("the section is retired ({})", s.why)));
                        }
                        (Stage::Superseded, Some(next)) => at = next,
                        (Stage::Superseded, None) => {
                            return Ok(Some(
                                "the section is superseded, naming nothing".to_owned(),
                            ));
                        }
                        _ => return Ok(None),
                    }
                }
            }
        }
        Ok(Some(
            "it leads round in a circle of superseded sections".to_owned(),
        ))
    }

    /// Every address a section cites, or a superseded one names, that does not resolve, found
    /// now. Nothing is stored: they are as true as the record this minute.
    ///
    /// # Errors
    /// A store failure.
    pub fn broken_roads(&self) -> Done<Vec<Road>> {
        let mut roads = Vec::new();
        for s in self.store.sections()? {
            let from = s.address();
            let mut targets = if s.stage.current() {
                map::cited(&format!("{} {}", s.headline, s.body))
            } else {
                Vec::new()
            };
            if s.stage == Stage::Superseded {
                targets.extend(s.superseded_by.clone());
            }
            for to in targets.into_iter().filter(|t| *t != from) {
                if let Some(why) = self.unresolved(&to)? {
                    roads.push(Road {
                        from: from.clone(),
                        to,
                        why,
                    });
                }
            }
        }
        Ok(roads)
    }

    /// The routes out of a section, for when it is read: each address it cites, and whether it
    /// leads anywhere.
    pub(super) fn routes_said(&self, headline: &str, body: &str, from: &str) -> Done<String> {
        let cited: Vec<String> = map::cited(&format!("{headline} {body}"))
            .into_iter()
            .filter(|c| c != from)
            .collect();
        if cited.is_empty() {
            return Ok(String::new());
        }
        let mut routes = Vec::new();
        for to in cited {
            routes.push(match self.unresolved(&to)? {
                None => format!("{to} (leads there)"),
                Some(why) => format!("{to} (BROKEN: {why})"),
            });
        }
        Ok(format!("Routes out: {}.\n", routes.join("; ")))
    }

    /// The broken roads out of pages `member` keeps, for its briefing; empty when there are none.
    pub(super) fn roads_to_mend(&self, member: &Member) -> Done<String> {
        let mut mine = Vec::new();
        for road in self.broken_roads()? {
            let page = road
                .from
                .trim_start_matches("world:")
                .split('#')
                .next()
                .unwrap_or_default();
            let Some(key) = PageKey::parse(page) else {
                continue;
            };
            if self.steward(key)?.is(member) {
                mine.push(road);
            }
        }
        if mine.is_empty() {
            return Ok(String::new());
        }
        let mut said = String::from(
            "Broken roads out of pages you keep (an address cited there does not resolve; mend \
             the cite, or write what is missing):\n",
        );
        for road in mine.iter().take(LISTED) {
            said.push_str(&format!(
                "- {} cites {}: {}.\n",
                road.from, road.to, road.why
            ));
        }
        if mine.len() > LISTED {
            said.push_str(&format!("- and {} more.\n", mine.len() - LISTED));
        }
        Ok(said)
    }
}
