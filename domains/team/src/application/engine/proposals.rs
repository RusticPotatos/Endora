//! Proposals to a page (ADR 0126): a member who does not keep a page proposes a section to it, on
//! evidence from the record, and the page's steward writes it or declines it, saying why. Code
//! enforces who may decide; the proposer is told how it went, once, in its next briefing.

use serde_json::json;

use super::world::WorldWrite;
use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::map::{PROPOSALS_OPEN_MOST, PageKey, PageProposal, ProposalStatus};
use crate::domain::world::{self, Stage, WorldRefusal};
use crate::domain::{EventKind, Member, MemberId, Ref, Refusal, RunId};

/// A section a member proposes to a page it does not keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageProposing {
    /// The page: `area/3`, `project/2`, `role/developer`, `house`.
    pub page: String,
    /// The section it would revise, by anchor; none for a new one.
    pub anchor: Option<String>,
    /// Its heading.
    pub heading: String,
    /// What a briefing shows of it, in one line.
    pub headline: String,
    /// The rest of it.
    pub body: String,
    /// What it rests on: at least one reference in the record.
    pub evidence: Vec<Ref>,
}

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

/// The most proposals a briefing lists before it says how many more there are.
const LISTED: usize = 5;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    fn proposal(&self, id: i64) -> Done<PageProposal> {
        self.store
            .proposals()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| refused(WorldRefusal::NoSuchProposal(id)))
    }

    fn proposer_called(&self, by: MemberId) -> Done<String> {
        Ok(self.member(by)?.name)
    }

    /// A turn proposes a section to a page someone else keeps. Returns the proposal's number.
    /// The same proposal again is that one; a member may have five open at once.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::NoSuchPage`]; [`WorldRefusal::Derived`];
    /// [`WorldRefusal::YourOwnPage`] for a page the proposer keeps; [`WorldRefusal::Nothing`] for
    /// a section to revise that is not there; a heading, headline or body that does not keep;
    /// what the evidence rule refuses; [`WorldRefusal::TooManyProposals`].
    pub fn propose_world_in(&self, run: RunId, proposing: &PageProposing) -> Done<i64> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let key = self.written_page(&Self::slug_said(&proposing.page))?;
        let slug = key.slug();
        if self.steward(key)?.is(&who) {
            return Err(refused(WorldRefusal::YourOwnPage(slug)));
        }
        let said = |text: &str| self.dated_true(&who.name, &format!("world:{slug}"), text);
        let heading =
            world::one_line(&said(&proposing.heading)?, world::HEADING_MOST).map_err(refused)?;
        let headline =
            world::one_line(&said(&proposing.headline)?, world::HEADLINE_MOST).map_err(refused)?;
        let body = world::body(&said(&proposing.body)?).map_err(refused)?;
        let anchor = match proposing.anchor.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(a) => Some(
                world::anchor_of(a)
                    .ok_or_else(|| refused(WorldRefusal::Nothing(format!("world:{slug}#{a}"))))?,
            ),
        };
        let heading = match &anchor {
            Some(a) => {
                let old = self
                    .section_at(&slug, a)?
                    .ok_or_else(|| refused(WorldRefusal::Nothing(format!("world:{slug}#{a}"))))?;
                if heading.is_empty() {
                    old.heading
                } else {
                    heading
                }
            }
            None => heading,
        };
        if heading.is_empty() || headline.is_empty() {
            return Err(refused(WorldRefusal::NeedsHeading));
        }
        // A claim the owner retracted is not proposed again (ADR 0124).
        self.not_retracted(&format!("{heading} {headline} {body}"))?;
        let found = self.found_all(&proposing.evidence)?;
        world::warranted(Stage::Observation, &found, 0, "", turn.id)
            .map_err(EngineError::Refused)?;
        let open: Vec<PageProposal> = self
            .store
            .proposals()?
            .into_iter()
            .filter(|p| p.status == ProposalStatus::Open && p.by == who.id)
            .collect();
        let same = open.iter().find(|p| {
            p.page == slug
                && match &anchor {
                    Some(a) => p.anchor.as_ref() == Some(a),
                    None => p.anchor.is_none() && p.heading == heading,
                }
        });
        if let Some(same) = same {
            return Ok(same.id);
        }
        if open.len() >= PROPOSALS_OPEN_MOST {
            return Err(refused(WorldRefusal::TooManyProposals(open.len())));
        }
        let id = self.store.save_proposal(&PageProposal {
            id: 0,
            page: slug.clone(),
            anchor,
            heading: heading.clone(),
            headline,
            body,
            evidence: self.snapshots(&proposing.evidence)?,
            by: who.id,
            run: Some(turn.id),
            at: self.clock.now(),
            status: ProposalStatus::Open,
            decided_by: String::new(),
            why: String::new(),
            decided_at: None,
            told: false,
        })?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "proposed to a page", "proposal": id, "page": slug,
                    "by": who.name, "heading": heading }),
        )?;
        if let Some(task) = turn.task {
            self.mark_progress(who.id, task)?;
        }
        Ok(id)
    }

    /// Whether `who` decides what is proposed to the page.
    fn may_decide(&self, who: &Member, key: PageKey) -> Done<()> {
        let steward = self.steward(key)?;
        if steward.is(who) {
            return Ok(());
        }
        Err(refused(WorldRefusal::DecidedBy {
            what: self.page_title(key)?,
            steward: self.steward_called(steward)?,
        }))
    }

    /// The steward's turn decides a proposal: it writes the section, as an observation or (on
    /// evidence that warrants it) an insight, or declines it, saying why. Only the page's steward
    /// decides; a canon section is the owner's to replace. Returns the section's address when it
    /// was written.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::NoSuchProposal`]; [`WorldRefusal::ProposalDecided`];
    /// [`WorldRefusal::DecidedBy`]; [`WorldRefusal::NeedsWhy`] for a decline with no reason; what
    /// writing the section refuses.
    pub fn decide_proposal_in(
        &self,
        run: RunId,
        id: i64,
        accept: bool,
        reason: &str,
        stage: Stage,
        warrant: &str,
    ) -> Done<Option<String>> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let mut proposal = self.proposal(id)?;
        if proposal.status != ProposalStatus::Open {
            return Err(refused(WorldRefusal::ProposalDecided(id)));
        }
        let key = self.page_there(&proposal.page)?;
        self.may_decide(&who, key)?;
        let why = world::one_line(reason, world::WHY_MOST).map_err(refused)?;
        if !accept && why.is_empty() {
            return Err(refused(WorldRefusal::NeedsWhy));
        }
        let written = if accept {
            let proposer = self.proposer_called(proposal.by)?;
            let write = WorldWrite {
                page: proposal.page.clone(),
                anchor: proposal.anchor.clone(),
                heading: proposal.heading.clone(),
                headline: proposal.headline.clone(),
                body: proposal.body.clone(),
                stage,
                warrant: warrant.to_owned(),
                evidence: world::Snapshot::refs(&proposal.evidence),
            };
            Some(self.write_world_held(
                &turn,
                &who,
                &write,
                Some(&format!("{proposer}'s proposal {id}")),
            )?)
        } else {
            None
        };
        self.settle_proposal(&mut proposal, &who.name, &why, written.as_deref())?;
        Ok(written)
    }

    /// The owner decides a proposal to any page: accepted, it is written as the owner's word (an
    /// insight at least); declined, it needs a reason. Returns the section's address when it was
    /// written.
    ///
    /// # Errors
    /// [`WorldRefusal::NoSuchProposal`]; [`WorldRefusal::ProposalDecided`];
    /// [`WorldRefusal::NeedsWhy`] for a decline with no reason; what writing the section refuses.
    pub fn decide_proposal_as_owner(
        &self,
        id: i64,
        accept: bool,
        reason: &str,
    ) -> Done<Option<String>> {
        let _held = self.held();
        let mut proposal = self.proposal(id)?;
        if proposal.status != ProposalStatus::Open {
            return Err(refused(WorldRefusal::ProposalDecided(id)));
        }
        let why = world::one_line(reason, world::WHY_MOST).map_err(refused)?;
        if !accept && why.is_empty() {
            return Err(refused(WorldRefusal::NeedsWhy));
        }
        let written = if accept {
            let text = if proposal.body.is_empty() {
                proposal.headline.clone()
            } else {
                format!("{}\n{}", proposal.headline, proposal.body)
            };
            let key = proposal
                .anchor
                .clone()
                .unwrap_or_else(|| proposal.heading.clone());
            let address =
                self.write_as_owner_held(&proposal.page, &key, &proposal.heading, &text)?;
            let proposer = self.proposer_called(proposal.by)?;
            self.timeline_line(
                &proposal.page,
                "the owner",
                None,
                &format!("The owner accepted {proposer}'s proposal {id} ({address})."),
            )?;
            Some(address)
        } else {
            None
        };
        self.settle_proposal(&mut proposal, "the owner", &why, written.as_deref())?;
        Ok(written)
    }

    /// Records a proposal's decision, and says so in the feed.
    fn settle_proposal(
        &self,
        proposal: &mut PageProposal,
        by: &str,
        why: &str,
        written: Option<&str>,
    ) -> Done<()> {
        proposal.status = if written.is_some() {
            ProposalStatus::Accepted
        } else {
            ProposalStatus::Declined
        };
        proposal.decided_by = by.to_owned();
        proposal.why = why.to_owned();
        proposal.decided_at = Some(self.clock.now());
        self.store.save_proposal(proposal)?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": if written.is_some() { "proposal accepted" } else { "proposal declined" },
                    "proposal": proposal.id, "page": proposal.page, "by": by, "why": why,
                    "address": written }),
        )
    }

    // ---- what a briefing says of them ------------------------------------------------------

    /// The open proposals to pages `member` keeps, for its briefing; empty when there are none.
    pub(super) fn proposals_to_decide(&self, member: &Member) -> Done<String> {
        let mut mine = Vec::new();
        for p in self.store.proposals()? {
            if p.status != ProposalStatus::Open {
                continue;
            }
            let Some(key) = PageKey::parse(&p.page) else {
                continue;
            };
            if self.steward(key)?.is(member) {
                mine.push(p);
            }
        }
        if mine.is_empty() {
            return Ok(String::new());
        }
        let mut said = String::from(
            "Proposals to pages you keep, waiting for your decision (decide_proposal {id, accept, \
             reason}; a decline says why):\n",
        );
        for p in mine.iter().take(LISTED) {
            let by = self.proposer_called(p.by)?;
            let rests: Vec<String> = p.evidence.iter().map(|e| e.cited.said()).collect();
            let revising = p
                .anchor
                .as_ref()
                .map_or(String::new(), |a| format!(" (revising #{a})"));
            said.push_str(&format!(
                "- #{} {by} proposes on world:{}{revising}: \"{}\" \u{2014} {} It rests on: {}.\n",
                p.id,
                p.page,
                p.heading,
                p.headline,
                rests.join(", ")
            ));
        }
        if mine.len() > LISTED {
            said.push_str(&format!("- and {} more.\n", mine.len() - LISTED));
        }
        Ok(said)
    }

    /// How `member`'s proposals went, once, for its next briefing; empty when none is news.
    pub(super) fn proposals_decided(&self, member: &Member) -> Done<String> {
        let mut said = String::new();
        for p in self.store.proposals()? {
            if p.by != member.id || p.told || p.status == ProposalStatus::Open {
                continue;
            }
            let how = if p.status == ProposalStatus::Accepted {
                format!("accepted by {}", p.decided_by)
            } else {
                format!("declined by {}: {}", p.decided_by, p.why)
            };
            said.push_str(&format!(
                "- #{} to world:{} (\"{}\") was {how}.\n",
                p.id, p.page, p.heading
            ));
        }
        if said.is_empty() {
            return Ok(said);
        }
        Ok(format!("Your proposals to pages, decided:\n{said}"))
    }

    /// Marks `member`'s decided proposals as told.
    pub(super) fn proposals_told(&self, member: &Member) -> Done<()> {
        for mut p in self.store.proposals()? {
            if p.by == member.id && !p.told && p.status != ProposalStatus::Open {
                p.told = true;
                self.store.save_proposal(&p)?;
            }
        }
        Ok(())
    }
}
