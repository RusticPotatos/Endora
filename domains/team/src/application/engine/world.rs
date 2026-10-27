//! The team's world (ADR 0122, amended by ADR 0142): the house page and its timeline, the
//! derived lab and team pages, and handover notes. Members read it; the owner writes it.

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::evidence::Found;
use crate::domain::map::{self, PageKey};
use crate::domain::world::{
    self, Address, HOUSE, HandoverNote, Section, Snapshot, Stage, Trust, WorldRefusal,
};
use crate::domain::world_migration::TimelineLine;
use crate::domain::{EventKind, Member, MemberId, Millis, Refusal, RunId};

/// The day of `at` with its year, in the owner's time: `17 Oct 2026`.
pub(super) fn day_with_year(at: Millis, offset_minutes: i64) -> String {
    let day = crate::domain::money::day_of(at, offset_minutes);
    let (year, _, _) = crate::domain::lessons::civil(day);
    let (d, m) = crate::domain::lessons::day_and_month(day);
    format!("{d} {m} {year}")
}

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the move of the old record (once) ------------------------------------------------

    /// Whether the old record has been moved into the world.
    pub(super) fn world_migrated(&self) -> Done<bool> {
        Ok(self.store.setting("world_migrated")?.is_some())
    }

    /// A goal's notes as a briefing reads them.
    pub(super) fn goal_note_briefed(&self, goal: crate::domain::GoalId) -> Done<String> {
        Ok(self.store.goal_note(goal)?)
    }

    // ---- who writes what ------------------------------------------------------------------

    // ---- evidence and trust ---------------------------------------------------------------

    /// Whether any of a piece's evidence is still in the record (a piece with none, the owner's
    /// own word, needs none).
    fn resolves(&self, evidence: &[Snapshot]) -> Done<bool> {
        if evidence.is_empty() {
            return Ok(true);
        }
        for s in evidence {
            if self.found(s.cited, None, &[])? != Found::Missing {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// How far a section is trusted now.
    pub(super) fn section_trust(&self, s: &Section) -> Done<Trust> {
        let horizon = PageKey::parse(&s.page).map_or(HOUSE.horizon_days, PageKey::horizon_days);
        Ok(world::trust(
            s.stage,
            s.verified_at.unwrap_or(s.updated_at),
            horizon,
            self.clock.now(),
            self.resolves(&s.evidence)?,
        ))
    }

    /// A stage as shown: `insight`, or `observation, unconfirmed since 17 Sep`.
    pub(super) fn stage_said(&self, trust: Trust) -> String {
        match trust.unconfirmed_since {
            Some(since) => format!("unconfirmed since {}", self.said_day(since)),
            None => trust.stage.said().to_owned(),
        }
    }

    // ---- finding things -------------------------------------------------------------------

    /// A page as a member names it, as a slug: `world:Area/3` is `area/3`.
    pub(super) fn slug_said(said: &str) -> String {
        let said = said.trim().to_ascii_lowercase();
        said.strip_prefix("world:").unwrap_or(&said).to_owned()
    }

    pub(super) fn address(said: &str) -> Done<Address> {
        Address::parse(said).ok_or_else(|| refused(WorldRefusal::NotAnAddress(said.to_owned())))
    }

    pub(super) fn section_at(&self, page: &str, anchor: &str) -> Done<Option<Section>> {
        Ok(self
            .store
            .sections()?
            .into_iter()
            .find(|s| s.page == page && s.anchor == anchor))
    }

    /// A member's text with the year of a date it starts with corrected when the year is out
    /// by one while the day and month are today's, give or take three days. The correction
    /// is recorded in the feed.
    pub(super) fn dated_true(&self, who: &str, what: &str, text: &str) -> Done<String> {
        let today = crate::domain::money::day_of(self.clock.now(), self.settings.offset_minutes);
        let Some((fixed, was)) = crate::domain::house::corrected_year(text, today) else {
            return Ok(text.to_owned());
        };
        let (now, _, _) = crate::domain::lessons::civil(today);
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "year corrected", "by": who, "in": what, "was": was, "now": now }),
        )?;
        Ok(fixed)
    }

    /// Adds a line to a page's timeline, now.
    pub(super) fn timeline_line(
        &self,
        page: &str,
        by: &str,
        run: Option<RunId>,
        line: &str,
    ) -> Done<()> {
        let line: String = line.chars().take(world::TIMELINE_LINE_MOST).collect();
        self.store.add_timeline(
            &TimelineLine {
                page: page.to_owned(),
                at: self.clock.now(),
                by: by.to_owned(),
                line,
            },
            run,
        )?;
        Ok(())
    }

    /// The house page's timeline gains a member's dated line: what the house log's `note_house`
    /// does now (ADR 0122). Returns the line, dated.
    pub(super) fn note_house_line(&self, run: &Run, ceo: &Member, entry: &str) -> Done<String> {
        let entry = self.dated_true(&ceo.name, "the house page's timeline", entry)?;
        let line =
            crate::domain::house::line(&entry, self.clock.now(), self.settings.offset_minutes)
                .map_err(EngineError::Refused)?;
        let said = line
            .split_once(": ")
            .map_or(line.as_str(), |(_, rest)| rest)
            .to_owned();
        self.timeline_line(HOUSE.slug, &ceo.name, Some(run.id), &said)?;
        self.emit(
            EventKind::HouseNoted,
            json!({ "by": ceo.name, "entry": line, "page": HOUSE.slug }),
        )?;
        Ok(line)
    }

    // ---- what a turn may do ---------------------------------------------------------------

    /// A turn reads a page, a section or a story of the world. What it reads counts as used.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::NotAnAddress`]; [`WorldRefusal::Nothing`].
    pub fn read_world_in(&self, run: RunId, address: &str) -> Done<String> {
        let _held = self.held();
        self.running(run)?;
        let at = Self::address(address)?;
        let now = self.clock.now();
        match &at {
            Address::Page(page) => {
                if !matches!(page.as_str(), "lab" | "team") {
                    self.unclassified_message_input(run)?;
                }
                self.page_said(page)
            }
            Address::Section { page, anchor } => {
                self.page_there(page)?;
                let s = self
                    .section_at(page, anchor)?
                    .ok_or_else(|| refused(WorldRefusal::Nothing(at.said())))?;
                self.inherit_input_from(run, s.written_in_run)?;
                self.store.touch_section(s.id, now)?;
                self.section_said(&s)
            }
        }
    }

    fn evidence_lines(evidence: &[Snapshot]) -> String {
        if evidence.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = evidence
            .iter()
            .map(|e| {
                if e.said.is_empty() {
                    e.cited.said()
                } else {
                    format!("{} ({})", e.cited.said(), e.said)
                }
            })
            .collect();
        format!("It rests on: {}\n", lines.join("; "))
    }

    /// A section, whole, as `read_world` gives it.
    fn section_said(&self, s: &Section) -> Done<String> {
        let trust = self.section_trust(s)?;
        let mut said = format!(
            "{} ({}): {} [{}]\n{}\n",
            s.heading,
            s.address(),
            s.headline,
            self.stage_said(trust),
            s.body
        );
        if !s.warrant.is_empty() {
            said.push_str(&format!("Warrant: {}\n", s.warrant));
        }
        said.push_str(&Self::evidence_lines(&s.evidence));
        said.push_str(&self.routes_said(&s.headline, &s.body, &s.address())?);
        if let Some(next) = &s.superseded_by {
            said.push_str(&format!("Superseded by {next}.\n"));
        }
        if !s.why.is_empty() {
            said.push_str(&format!("{}\n", s.why));
        }
        said.push_str(&format!(
            "Revision {}, by {}, {}.\n",
            s.revision,
            s.written_by,
            self.said_day(s.updated_at)
        ));
        Ok(said)
    }

    /// A page, as `read_world` gives it: a written page's sections and the newest of its
    /// timeline; a derived page as HQ derives it now.
    fn page_said(&self, page: &str) -> Done<String> {
        if page == world::LAB.slug {
            return Ok(self.lab_said());
        }
        if page == world::TEAM.slug {
            return self.team_said();
        }
        let key = self.page_there(page)?;
        let mut said = format!(
            "{} (world:{page}):\nKept by {}.\n",
            self.page_title(key)?,
            map::steward_of(key).said()
        );
        let sections: Vec<Section> = self
            .store
            .sections()?
            .into_iter()
            .filter(|s| s.page == page)
            .collect();
        if sections.is_empty() {
            said.push_str("No sections yet.\n");
        }
        for s in &sections {
            let trust = self.section_trust(s)?;
            said.push_str(&format!(
                "- {} [{}] ({})\n",
                s.headline,
                self.stage_said(trust),
                s.address()
            ));
        }
        let timeline = self.store.timeline(page)?;
        if !timeline.is_empty() {
            said.push_str("Its timeline, newest last:\n");
            for line in timeline.iter().rev().take(20).rev() {
                said.push_str(&format!(
                    "- {}, {}: {}\n",
                    day_with_year(line.at, self.settings.offset_minutes),
                    line.by,
                    line.line
                ));
            }
        }
        Ok(said)
    }

    /// The lab page, derived from code (ADR 0112).
    pub(super) fn lab_said(&self) -> String {
        let mut said = format!("{} (world:lab):\n", world::LAB.title);
        for line in endora_kernel::lab::LAB_BUILT {
            said.push_str(&format!("- {line}\n"));
        }
        said
    }

    /// The team page, derived from the record: who is at work, in what role, on what brain,
    /// reporting to whom.
    pub(super) fn team_said(&self) -> Done<String> {
        let members = self.store.members()?;
        let mut said = format!("{} (world:team):\n", world::TEAM.title);
        for m in &members {
            let to = m
                .reports_to
                .and_then(|id| members.iter().find(|o| o.id == id))
                .map(|o| format!(", reports to {}", o.name))
                .unwrap_or_default();
            let away = if m.furloughed() {
                ", on furlough"
            } else if m.paused {
                ", paused"
            } else {
                ""
            };
            said.push_str(&format!(
                "- {} ({}, {}{to}{away})\n",
                m.name,
                m.role.title(),
                m.brain.label()
            ));
        }
        Ok(said)
    }

    /// The owner writes a section of a written page, or replaces one: its heading, a headline
    /// (the text's first line) and the rest as its body. It stands as an insight at least, with
    /// no evidence asked (the owner is the warrant), canon if it was; the revision it replaces
    /// stays in the section's history. A date the text starts with is mended as a member's is.
    /// Returns its address.
    ///
    /// # Errors
    /// [`WorldRefusal::NotAnAddress`]; [`WorldRefusal::Derived`]; [`WorldRefusal::NeedsHeading`]
    /// for no title or no text; [`WorldRefusal::TooLong`].
    pub fn write_as_owner(&self, page: &str, key: &str, title: &str, text: &str) -> Done<String> {
        let _held = self.held();
        self.write_as_owner_held(page, key, title, text)
    }

    /// [`write_as_owner`](Self::write_as_owner), with the lock held.
    pub(super) fn write_as_owner_held(
        &self,
        page: &str,
        key: &str,
        title: &str,
        text: &str,
    ) -> Done<String> {
        let slug = self.written_page(&Self::slug_said(page))?.slug();
        let owner = "the owner";
        let text = self.dated_true(owner, &format!("world:{slug}"), text)?;
        let heading = world::one_line(title, world::HEADING_MOST).map_err(refused)?;
        let anchor = world::anchor_of(if key.trim().is_empty() { &heading } else { key })
            .ok_or_else(|| refused(WorldRefusal::NeedsHeading))?;
        let text = world::body(&text).map_err(refused)?;
        let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
        let headline = world::one_line(first, usize::MAX).map_err(refused)?;
        let headline: String = headline.chars().take(world::HEADLINE_MOST).collect();
        if heading.is_empty() || headline.is_empty() {
            return Err(refused(WorldRefusal::NeedsHeading));
        }
        // A headline cut short keeps its end in the body, so nothing written is lost.
        let body = if headline.chars().count() < first.trim().chars().count() {
            text.clone()
        } else {
            rest.trim().to_owned()
        };
        let old = self.section_at(&slug, &anchor)?;
        let now = self.clock.now();
        let section = Section {
            id: old.as_ref().map_or(0, |s| s.id),
            page: slug.clone(),
            anchor,
            heading,
            headline,
            body,
            stage: if old.as_ref().is_some_and(|s| s.stage == Stage::Canon) {
                Stage::Canon
            } else {
                Stage::Insight
            },
            warrant: "Written by the owner.".to_owned(),
            evidence: Vec::new(),
            verified_at: Some(now),
            verified_by: owner.to_owned(),
            superseded_by: None,
            why: String::new(),
            written_by: owner.to_owned(),
            written_in_run: None,
            revision: old.as_ref().map_or(1, |s| s.revision + 1),
            updated_at: now,
            last_used_at: now,
        };
        let address = section.address();
        let what = if old.is_some() { "revised" } else { "wrote" };
        self.store
            .save_section(&section, &format!("the owner {what} it"))?;
        self.timeline_line(
            &slug,
            owner,
            None,
            &format!("The owner {what} \"{}\" ({address}).", section.heading),
        )?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "owner wrote", "address": address, "by": "owner",
                    "revision": section.revision }),
        )?;
        Ok(address)
    }

    /// The owner adds a dated entry to a written page's timeline: at `at`, else now. A date
    /// the text starts with is mended if its year is out, then taken off: the entry's date is
    /// HQ's.
    ///
    /// # Errors
    /// [`WorldRefusal::NotAnAddress`]; [`WorldRefusal::Derived`]; [`WorldRefusal::NeedsHeading`]
    /// for no text.
    pub fn add_to_timeline_as_owner(&self, page: &str, at: Option<Millis>, text: &str) -> Done<()> {
        let _held = self.held();
        let slug = self.written_page(&Self::slug_said(page))?.slug();
        let owner = "the owner";
        let text = self.dated_true(owner, &format!("world:{slug}"), text)?;
        let line = world::one_line(
            crate::domain::house::without_dates(&text),
            world::TIMELINE_LINE_MOST,
        )
        .map_err(refused)?;
        if line.is_empty() {
            return Err(refused(WorldRefusal::NeedsHeading));
        }
        self.store.add_timeline(
            &TimelineLine {
                page: slug.clone(),
                at: at.unwrap_or_else(|| self.clock.now()),
                by: owner.to_owned(),
                line: line.clone(),
            },
            None,
        )?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "owner noted", "page": slug, "entry": line, "by": "owner" }),
        )?;
        Ok(())
    }

    /// After a member's post: the sections it names by address were used.
    pub(super) fn world_used(&self, text: &str) -> Done<()> {
        let now = self.clock.now();
        let lower = text.to_ascii_lowercase();
        for s in self.store.sections()? {
            if lower.contains(&s.address()) {
                self.store.touch_section(s.id, now)?;
            }
        }
        Ok(())
    }

    // ---- handing work on ------------------------------------------------------------------

    /// Keeps the handover note of work handed on in a turn, and says so in the feed.
    pub(super) fn keep_handover(
        &self,
        turn: &Run,
        task: crate::domain::TaskId,
        from: Option<MemberId>,
        to: Option<MemberId>,
        note: &HandoverNote,
    ) -> Done<()> {
        self.store.add_handover(&world::Handover {
            id: 0,
            task,
            by: turn.member,
            from,
            to,
            run: Some(turn.id),
            note: note.clone(),
            evidence: Vec::new(),
            at: self.clock.now(),
        })?;
        self.emit(
            EventKind::HandedOver,
            json!({ "task": task.0, "by": turn.member.0, "from": from.map(|m| m.0),
                    "to": to.map(|m| m.0), "note": note.line() }),
        )
    }

    // ---- what the owner sees --------------------------------------------------------------

    /// The world as the HQ screen shows it: the pages by stage, with their timelines, and the
    /// stories.
    pub fn world_shown(&self) -> Done<Value> {
        let off = self.settings.offset_minutes;
        let snaps = |all: &[Snapshot]| -> Vec<Value> {
            all.iter()
                .map(|e| json!({ "ref": e.cited.said(), "said": e.said }))
                .collect()
        };
        let mut sections = Vec::new();
        for s in self.store.sections()? {
            let trust = self.section_trust(&s)?;
            sections.push(json!({
                "address": s.address(), "page": s.page, "anchor": s.anchor, "heading": s.heading,
                "headline": s.headline, "body": s.body, "stage": s.stage.name(),
                "shown": trust.stage.name(), "said": self.stage_said(trust),
                "unconfirmed_since": trust.unconfirmed_since, "warrant": s.warrant,
                "evidence": snaps(&s.evidence), "verified_at": s.verified_at,
                "verified_by": s.verified_by, "revision": s.revision, "updated_at": s.updated_at,
                "written_by": s.written_by, "why": s.why, "superseded_by": s.superseded_by,
            }));
        }
        let timeline: Vec<Value> = self
            .store
            .timeline(HOUSE.slug)?
            .iter()
            .rev()
            .take(50)
            .map(|l| {
                json!({ "at": l.at, "day": day_with_year(l.at, off), "by": l.by,
                             "line": l.line })
            })
            .collect();
        Ok(json!({
            "pages": [
                { "slug": HOUSE.slug, "title": HOUSE.title, "derived": false,
                  "horizon_days": HOUSE.horizon_days, "timeline": timeline },
                { "slug": world::LAB.slug, "title": world::LAB.title, "derived": true,
                  "lines": endora_kernel::lab::LAB_BUILT },
                { "slug": world::TEAM.slug, "title": world::TEAM.title, "derived": true,
                  "lines": self.team_said()?.lines().skip(1).map(|l| l.trim_start_matches("- ").to_owned()).collect::<Vec<_>>() },
            ],
            "sections": sections,
            "migrated": self.world_migrated()?,
            "map": self.map_shown()?,
        }))
    }

    /// A written page's sections and its timeline, newest first, as Markdown.
    fn written_page_markdown(&self, slug: &str) -> Done<String> {
        let off = self.settings.offset_minutes;
        let mut md = String::new();
        let sections: Vec<Section> = self
            .store
            .sections()?
            .into_iter()
            .filter(|s| s.page == slug)
            .collect();
        if sections.is_empty() {
            md.push_str("No sections yet.\n\n");
        }
        for s in &sections {
            let trust = self.section_trust(s)?;
            md.push_str(&format!(
                "### {} {{#{}}}\n\n**{}**\n\nStage: {} · revision {}, by {}, {}\n\n",
                s.heading,
                s.anchor,
                s.headline,
                self.stage_said(trust),
                s.revision,
                s.written_by,
                day_with_year(s.updated_at, off)
            ));
            if !s.body.is_empty() {
                md.push_str(&format!("{}\n\n", s.body));
            }
            if !s.warrant.is_empty() {
                md.push_str(&format!("Warrant: {}\n\n", s.warrant));
            }
            let lines = Self::evidence_lines(&s.evidence);
            if !lines.is_empty() {
                md.push_str(&format!("{lines}\n"));
            }
            if !s.why.is_empty() {
                md.push_str(&format!("{}\n\n", s.why));
            }
        }
        md.push_str("### Timeline (newest first)\n\n");
        for l in self.store.timeline(slug)?.into_iter().rev() {
            md.push_str(&format!(
                "- {}, {}: {}\n",
                day_with_year(l.at, off),
                l.by,
                l.line
            ));
        }
        md.push('\n');
        Ok(md)
    }

    /// The world as one Markdown document, for the owner to download and keep.
    pub fn world_markdown(&self) -> Done<String> {
        let off = self.settings.offset_minutes;
        let now = self.clock.now();
        let mut md = format!(
            "# The team's world\n\nExported from Endora HQ on {}.\n\n## {} (world:house)\n\n",
            day_with_year(now, off),
            HOUSE.title
        );
        md.push_str(&self.written_page_markdown(HOUSE.slug)?);
        // The pages of projects and goals (ADR 0126), each with its steward.
        for slug in self.store.world_pages()? {
            let Some(key) = PageKey::parse(&slug) else {
                continue;
            };
            let kept = self.written_page_markdown(&slug)?;
            if kept.starts_with("No sections yet.\n\n### Timeline (newest first)\n\n\n") {
                continue;
            }
            md.push_str(&format!(
                "## {} (world:{slug})\n\nKept by {}.\n\n{kept}",
                self.page_title(key)?,
                map::steward_of(key).said()
            ));
        }
        // A derived page: its heading, then its lines (its first line is the heading).
        md.push_str(&format!(
            "## {} (world:{})\n\n",
            world::LAB.title,
            world::LAB.slug
        ));
        for line in self.lab_said().lines().skip(1) {
            md.push_str(&format!("{line}\n"));
        }
        md.push_str(&format!(
            "\n## {} (world:{})\n\n",
            world::TEAM.title,
            world::TEAM.slug
        ));
        for line in self.team_said()?.lines().skip(1) {
            md.push_str(&format!("{line}\n"));
        }
        Ok(md)
    }

    /// What a turn was briefed: its situation, what of the world it carried and what was left
    /// out, against its budget.
    pub fn briefed(&self, run: RunId) -> Done<Option<crate::domain::situations::Briefed>> {
        Ok(self.store.briefed(run)?)
    }
}
