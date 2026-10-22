//! What a turn is told of the team's world (ADR 0122): its situation, decided from the record,
//! and the pieces that situation needs, composed within the member's budget. Whatever is left
//! out is named at the end of the briefing, and the run records the situation and what it was
//! told.

use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::situations::{self, Briefed, Composed, Facts, Piece, Situation, Weight};
use crate::domain::world::{HOUSE, Scope, Section, Stage, Story};
use crate::domain::{Member, Millis, Role};

/// The most characters of the house page's newest timeline lines a briefing carries while the
/// page has no sections yet.
const TIMELINE_CHARS: usize = 1_600;
/// The most lines of what changed a briefing carries after a gap.
const CHANGED_MOST: usize = 12;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The situation a turn starts from, from the record: back from a furlough, handed the
    /// work, new to it, after a gap, or continuing.
    pub(super) fn situation_of(&self, member: &Member, run: &Run) -> Done<Situation> {
        let runs = self.store.runs()?;
        let mine: Vec<&Run> = runs
            .iter()
            .filter(|r| r.member == member.id && r.id != run.id && r.started_at.is_some())
            .collect();
        let last_on_task = run.task.and_then(|task| {
            mine.iter()
                .filter(|r| r.task == Some(task))
                .filter_map(|r| r.started_at)
                .max()
        });
        // Work it now holds that others took turns on: being asked about a task, or testing it,
        // is not being handed it.
        let holds = match run.task {
            Some(task) => {
                let t = self.task(task)?;
                t.owner.or(t.assignee) == Some(member.id)
            }
            None => false,
        };
        let others_on_task = holds
            && run.task.is_some_and(|task| {
                runs.iter().any(|r| {
                    r.task == Some(task)
                        && r.member != member.id
                        && r.id != run.id
                        && r.started_at.is_some()
                })
            });
        let handed_a_note =
            match run.task {
                Some(task) => self.store.handovers(task)?.iter().any(|h| {
                    h.to == Some(member.id) && last_on_task.is_none_or(|last| h.at > last)
                }),
                None => false,
            };
        let first_in_project = match self.project_of(run.task)? {
            Some(project) => {
                let mut seen = false;
                for r in &mine {
                    if self.project_of(r.task)? == Some(project) {
                        seen = true;
                        break;
                    }
                }
                !seen
            }
            None => false,
        };
        Ok(situations::decide(&Facts {
            recalled: self.back_from_furlough(member, run)?.is_some(),
            handed_a_note,
            new_to_the_work: last_on_task.is_none() && others_on_task,
            first_turn: mine.is_empty(),
            first_in_project,
            last_on_task,
            last_turn: mine.iter().filter_map(|r| r.started_at).max(),
            now: self.clock.now(),
        }))
    }

    /// What of the world a member is told at the start of `run`, and its situation, composed
    /// within its budget. The run records it.
    pub(super) fn memory(&self, member: &Member, run: &Run) -> Done<(Situation, Composed)> {
        let situation = self.situation_of(member, run)?;
        let pieces = self.pieces(member, run, situation)?;
        let unclassified: Vec<String> = pieces
            .iter()
            .filter(|piece| {
                !piece.text.trim().is_empty()
                    && !matches!(
                        piece.name.as_str(),
                        "map" | "your notebook" | "tidy" | "furlough" | "world:lab"
                    )
            })
            .map(|piece| piece.name.clone())
            .collect();
        let composed = situations::compose(
            situations::budget(member.role, member.brain, situation),
            pieces,
        );
        if unclassified
            .iter()
            .any(|name| composed.briefed.contains(name))
        {
            self.unclassified_message_input(run.id)?;
        }
        self.store.save_briefed(&Briefed {
            run: run.id,
            situation,
            briefed: composed.briefed.clone(),
            dropped: composed.dropped.clone(),
            tokens: composed.tokens,
            budget: composed.budget,
            over_budget: composed.over_budget,
            at: self.clock.now(),
        })?;
        if composed.briefed.iter().any(|b| b == "tidy") {
            self.tidy_asked(member.id)?;
        }
        if composed.briefed.iter().any(|b| b == "your proposals") {
            self.proposals_told(member)?;
        }
        if composed.briefed.iter().any(|b| b == "retell") {
            self.store.set_setting(
                &super::world::retell_key(member.id),
                &self.clock.now().to_string(),
            )?;
        }
        Ok((situation, composed))
    }

    /// When the member's last turn before `run` started.
    fn last_turn_of(&self, member: &Member, run: &Run) -> Done<Option<Millis>> {
        Ok(self
            .store
            .runs()?
            .iter()
            .filter(|r| r.member == member.id && r.id != run.id)
            .filter_map(|r| r.started_at)
            .max())
    }

    /// The pieces a situation needs, in the order they are told: what the situation is about
    /// first (core), then the house page, the lab, the team, the stories and the notebook.
    fn pieces(&self, member: &Member, run: &Run, situation: Situation) -> Done<Vec<Piece>> {
        // The map opens it: the pages that matter to this member, by address (ADR 0126).
        let mut pieces = vec![Piece::new(
            "map",
            self.map_for(member, run)?.text,
            Weight::Core,
        )];
        let last = self.last_turn_of(member, run)?;
        let back = self.back_from_furlough(member, run)?;
        // What changed while it was away: since its furlough began, or its last turn.
        if matches!(situation, Situation::Recalled | Situation::AfterGap) {
            let since = match back {
                Some((from, _)) => from,
                None => last.unwrap_or(0),
            };
            pieces.push(Piece::new(
                "what changed",
                self.changed_since(member, since)?,
                Weight::Core,
            ));
        }
        if let Some((from, to)) = back {
            pieces.push(Piece::new(
                "furlough",
                self.furlough_said(from, to),
                Weight::Core,
            ));
        }
        if situation == Situation::Handover {
            pieces.push(Piece::new(
                "handover",
                self.handover_said(member, run)?,
                Weight::Core,
            ));
        }
        // Proposals to pages it keeps, and how its own went (ADR 0126).
        pieces.push(Piece::new(
            "proposals to decide",
            self.proposals_to_decide(member)?,
            Weight::Core,
        ));
        pieces.push(Piece::new(
            "your proposals",
            self.proposals_decided(member)?,
            Weight::Core,
        ));
        // The two ways the map fails, to those who keep the pages: what members looked for and
        // did not find, and the roads that lead nowhere (ADR 0126).
        pieces.push(Piece::new(
            "gaps",
            self.gaps_to_answer(member)?,
            Weight::Core,
        ));
        pieces.push(Piece::new(
            "broken roads",
            self.roads_to_mend(member)?,
            Weight::Core,
        ));
        pieces.push(Piece::new(
            "failing pages",
            self.quiz_flags_for(member)?,
            Weight::Core,
        ));
        pieces.extend(self.house_pieces(member, situation, last)?);
        // How the lab is built, for those who read the house log it was seeded in: a lead on
        // every turn, the CEO (whose briefings are held short) when it starts afresh or comes
        // back to its work.
        let wide = situation == Situation::Fresh || situation.dear();
        if member.role == Role::Lead || (member.role == Role::Ceo && wide) {
            pieces.push(Piece::new(
                "world:lab",
                self.lab_said(),
                Weight::Recommended,
            ));
        }
        pieces.extend(self.story_pieces(member, situation)?);
        if member.role == Role::Lead {
            pieces.push(Piece::new(
                "retell",
                self.retell_said(member)?,
                Weight::Recommended,
            ));
        }
        let (notes, tidy) = self.notebook_said(member, run.id)?;
        let notes_weight = if situation == Situation::Recalled || member.role == Role::Ceo {
            Weight::Core
        } else {
            Weight::Recommended
        };
        pieces.push(Piece::new("your notebook", notes, notes_weight));
        pieces.push(Piece::new("tidy", tidy, Weight::Recommended));
        Ok(pieces)
    }

    /// The house page's pieces for a member: the CEO and the leads its current sections (canon
    /// first) and what its timeline gained since their last turn, or its newest lines while it
    /// has no sections; everyone else its canon, and in a fresh start its insights too.
    fn house_pieces(
        &self,
        member: &Member,
        situation: Situation,
        last: Option<Millis>,
    ) -> Done<Vec<Piece>> {
        let mut pieces = Vec::new();
        let keeps_house = matches!(member.role, Role::Ceo | Role::Lead);
        let mut current: Vec<(Section, Stage, String)> = Vec::new();
        for s in self.store.sections()? {
            if s.page != HOUSE.slug || !s.stage.current() {
                continue;
            }
            let trust = self.section_trust(&s)?;
            current.push((s, trust.stage, self.stage_said(trust)));
        }
        // What the owner wrote comes first, then by stage.
        current.sort_by_key(|(s, stage, _)| {
            (s.written_by != "the owner", std::cmp::Reverse(*stage), s.id)
        });
        let header = match member.role {
            Role::Ceo => {
                "The house page (world:house), yours to keep with write_world and note_house; \
                 canon is the owner's:\n"
            }
            Role::Lead => {
                "The house page (world:house), kept by the CEO (read it; it is not yours to \
                 change):\n"
            }
            _ => "The house page's canon (world:house):\n",
        };
        let shown: Vec<&(Section, Stage, String)> = current
            .iter()
            .filter(|(s, stage, _)| {
                keeps_house
                    || s.written_by == "the owner"
                    || *stage == Stage::Canon
                    || (situation == Situation::Fresh && *stage == Stage::Insight)
            })
            .collect();
        if !keeps_house && shown.is_empty() {
            return Ok(pieces);
        }
        pieces.push(Piece::new("world:house", header, Weight::Core));
        for (s, stage, said) in &shown {
            let weight = match stage {
                Stage::Canon => Weight::Core,
                Stage::Insight => Weight::Recommended,
                _ => Weight::Optional,
            };
            pieces.push(Piece::new(
                s.address(),
                format!("- {} [{said}] ({})\n", s.headline, s.address()),
                weight,
            ));
        }
        if !keeps_house {
            return Ok(pieces);
        }
        let timeline = self.store.timeline(HOUSE.slug)?;
        let line = |l: &crate::domain::world_migration::TimelineLine| {
            format!(
                "- {}, {}: {}\n",
                super::world::day_with_year(l.at, self.settings.offset_minutes),
                l.by,
                l.line
            )
        };
        if current.is_empty() {
            if timeline.is_empty() {
                pieces.push(Piece::new(
                    "world:house",
                    "No sections yet.\n",
                    Weight::Core,
                ));
                return Ok(pieces);
            }
            let mut newest: Vec<String> = Vec::new();
            let mut chars = 0;
            for l in timeline.iter().rev() {
                let said = line(l);
                if !newest.is_empty() && chars + said.chars().count() > TIMELINE_CHARS {
                    break;
                }
                chars += said.chars().count();
                newest.push(said);
            }
            let more = timeline.len() - newest.len();
            // The house log's lines are on the timeline now (ADR 0122): the CEO, who keeps the
            // page, is asked to write what is true now from them.
            if member.role == Role::Ceo {
                pieces.push(Piece::new(
                    "world:house",
                    "No sections yet: write what is true of the house now, from its timeline, \
                     with write_world (as insights, on evidence).\n",
                    Weight::Core,
                ));
            }
            let mut text = String::from("No sections yet. Its timeline, newest last:\n");
            if more > 0 {
                text.push_str(&format!(
                    "- and {more} earlier lines (read_world world:house)\n"
                ));
            }
            text.extend(newest.into_iter().rev());
            pieces.push(Piece::new(
                "world:house timeline",
                text,
                Weight::Recommended,
            ));
            return Ok(pieces);
        }
        if let Some(last) = last {
            let new: Vec<String> = timeline.iter().filter(|l| l.at > last).map(line).collect();
            if !new.is_empty() {
                let mut text = String::from("New on its timeline since your last turn:\n");
                text.extend(new);
                pieces.push(Piece::new(
                    "world:house timeline",
                    text,
                    Weight::Recommended,
                ));
            }
        }
        Ok(pieces)
    }

    /// The stories a member is told: whole when its situation is one they are taught in, else
    /// one short line; current ones only, of the house, its area and its own.
    fn story_pieces(&self, member: &Member, situation: Situation) -> Done<Vec<Piece>> {
        let members = self.store.members()?;
        let seen = self.scopes_seen(member, &members);
        let mut pieces = Vec::new();
        for s in self.store.stories()? {
            if !seen.contains(&s.scope) || !s.stage.current() {
                continue;
            }
            let trust = self.story_trust(&s)?;
            let whole = !s.to_retell && s.taught.contains(&situation);
            let (text, weight) = if whole {
                (s.told(), Weight::Recommended)
            } else {
                (s.short(), Weight::Optional)
            };
            let mark = if matches!(s.stage, Stage::Canon | Stage::Insight) {
                format!(" [{}]", self.stage_said(trust))
            } else {
                String::new()
            };
            pieces.push(Piece::new(
                s.address(),
                format!("- Story {text}{mark}\n"),
                weight,
            ));
        }
        if !pieces.is_empty() {
            pieces.insert(
                0,
                Piece::new(
                    "stories",
                    "Stories: cite one by its handle (story:<handle>) instead of retelling it.\n",
                    Weight::Core,
                ),
            );
        }
        Ok(pieces)
    }

    /// A lead's ask, at most once a day, to retell up to three of its area's lessons as stories.
    fn retell_said(&self, lead: &Member) -> Done<String> {
        let asked = self
            .store
            .setting(&super::world::retell_key(lead.id))?
            .and_then(|at| at.parse::<Millis>().ok());
        if asked.is_some_and(|at| self.clock.now() - at < crate::domain::world::DAY_MS) {
            return Ok(String::new());
        }
        let due: Vec<Story> = self
            .store
            .stories()?
            .into_iter()
            .filter(|s| s.to_retell && s.scope == Scope::Area(lead.id))
            .take(super::world::RETELL_AT_ONCE)
            .collect();
        if due.is_empty() {
            return Ok(String::new());
        }
        let mut said = String::from(
            "Lessons of your area to retell as stories (tell_story with the same handle: when \
             it matters, the tempting move, what to do instead, and why, on evidence):\n",
        );
        for s in due {
            said.push_str(&format!("- {}: {}\n", s.handle, s.instead));
        }
        Ok(said)
    }

    /// The handover note a member is handed with its task, or that it is new to work others
    /// took turns on.
    fn handover_said(&self, member: &Member, run: &Run) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        let notes = self.store.handovers(task)?;
        let note = notes
            .iter()
            .rev()
            .find(|h| h.to == Some(member.id))
            .or_else(|| notes.last());
        if let Some(h) = note {
            // A note a member left itself when a restart ended its turn (ADR 0134).
            if h.by == member.id {
                return Ok(format!(
                    "Your own note, left on #{} when a restart ended your turn ({}). {}\n",
                    task.0,
                    self.said_day(h.at),
                    h.note.line()
                ));
            }
            let by = self.member(h.by)?.name;
            return Ok(format!(
                "Handed over by {by} on #{} ({}). {}\n",
                task.0,
                self.said_day(h.at),
                h.note.line()
            ));
        }
        Ok(format!(
            "You are new to #{}: others took turns on it before you. Read its thread before you \
             act.\n",
            task.0
        ))
    }

    /// What changed on the house page and in a member's stories since `since`.
    fn changed_since(&self, member: &Member, since: Millis) -> Done<String> {
        let mut lines: Vec<String> = Vec::new();
        for s in self.store.sections()? {
            if s.updated_at > since {
                lines.push(format!(
                    "- {} ({}), {}: {}\n",
                    s.heading,
                    s.address(),
                    s.stage.said(),
                    s.headline
                ));
            }
        }
        for l in self.store.timeline(HOUSE.slug)? {
            if l.at > since {
                lines.push(format!("- {}, {}: {}\n", self.said_day(l.at), l.by, l.line));
            }
        }
        let members = self.store.members()?;
        let seen = self.scopes_seen(member, &members);
        for s in self.store.stories()? {
            if seen.contains(&s.scope) && s.written_at > since {
                lines.push(format!("- Story {} was told: {}\n", s.handle, s.instead));
            }
        }
        let day = self.said_day(since);
        if lines.is_empty() {
            return Ok(format!(
                "Nothing on the house page or in your stories changed since {day}.\n"
            ));
        }
        let earlier = lines.len().saturating_sub(CHANGED_MOST);
        let mut said = format!("What changed since {day}:\n");
        if earlier > 0 {
            said.push_str(&format!(
                "- and {earlier} earlier changes (read_world world:house)\n"
            ));
        }
        said.extend(lines.into_iter().skip(earlier));
        Ok(said)
    }
}
