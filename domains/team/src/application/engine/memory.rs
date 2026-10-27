//! What a turn is told of the team's world (ADR 0122): its situation, decided from the record,
//! and the pieces that situation needs, composed within the member's budget. Whatever is left
//! out is named at the end of the briefing, and the run records the situation and what it was
//! told.

use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::situations::{self, Briefed, Composed, Facts, Piece, Situation, Weight};
use crate::domain::world::{HOUSE, Section, Stage};
use crate::domain::world_migration::TimelineLine;
use crate::domain::{Member, Millis, ProjectId, Role, RunId};

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
            Some(project) => !self.any_in_project(&mine, project)?,
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

    /// The turn inherits the lineage of each piece it was briefed: from the turns that wrote
    /// it, or unclassified where none is known.
    fn inherit_briefed(&self, member: &Member, run: &Run, briefed: &[&String]) -> Done<()> {
        if briefed.is_empty() {
            return Ok(());
        }
        let sections = self.store.sections()?;
        for name in briefed {
            for by in self.piece_written_in(member, run, name, &sections)? {
                match by {
                    Some(source) => self.inherit_input_from(run.id, Some(source))?,
                    None => self.unclassified_message_input(run.id)?,
                }
            }
        }
        Ok(())
    }

    /// Whether any of these turns was on work in `project`.
    fn any_in_project(&self, runs: &[&Run], project: ProjectId) -> Done<bool> {
        for r in runs {
            if self.project_of(r.task)? == Some(project) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The turns that wrote a briefed piece: the house timeline's lines by theirs, a section by
    /// its own; none known for any other piece.
    fn piece_written_in(
        &self,
        member: &Member,
        run: &Run,
        name: &str,
        sections: &[Section],
    ) -> Done<Vec<Option<RunId>>> {
        if name == "world:house timeline" {
            return self.house_timeline_runs(member, run, sections);
        }
        Ok(match sections.iter().find(|s| s.address() == name) {
            Some(section) => vec![section.written_in_run],
            None => vec![None],
        })
    }

    /// What of the world a member is told at the start of `run`, and its situation, composed
    /// within its budget. The run records it.
    pub(super) fn memory(&self, member: &Member, run: &Run) -> Done<(Situation, Composed)> {
        let situation = self.situation_of(member, run)?;
        let pieces = self.pieces(member, run, situation)?;
        let shown: Vec<String> = pieces
            .iter()
            .filter(|piece| {
                !piece.text.trim().is_empty()
                    && !matches!(
                        piece.name.as_str(),
                        // HQ's own words, or classified where they are read.
                        "map" | "furlough" | "world:lab" | "world:house"
                    )
            })
            .map(|piece| piece.name.clone())
            .collect();
        let composed = situations::compose(
            situations::budget(member.role, member.brain, situation),
            pieces,
        );
        // Each piece briefed is classified by the turns that wrote it (ADR 0138 A2d): a house
        // section, a story and the house's timeline lines by their recorded runs. What has none,
        // and every other piece, fails closed.
        let briefed: Vec<&String> = shown
            .iter()
            .filter(|name| composed.briefed.contains(name))
            .collect();
        self.inherit_briefed(member, run, &briefed)?;
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
    /// first (core), then the house page and the lab.
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
                self.changed_since(since)?,
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
        pieces.extend(self.house_pieces(member, situation, last)?);
        // How the lab is built, for every member (ADR 0142).
        pieces.push(Piece::new(
            "world:lab",
            self.lab_said(),
            Weight::Recommended,
        ));
        Ok(pieces)
    }

    /// The turns that wrote the house's timeline lines a briefing shows (`None` for a line no
    /// turn wrote): those since the member's last turn while the page has sections, else all of
    /// them, which holds the newest lines shown.
    fn house_timeline_runs(
        &self,
        member: &Member,
        run: &Run,
        sections: &[Section],
    ) -> Done<Vec<Option<crate::domain::RunId>>> {
        let current = sections
            .iter()
            .any(|s| s.page == HOUSE.slug && s.stage.current());
        let last = self.last_turn_of(member, run)?;
        Ok(self
            .store
            .timeline_with_runs(HOUSE.slug)?
            .into_iter()
            .filter(|(line, _)| !current || last.is_none_or(|last| line.at > last))
            .map(|(_, by)| by)
            .collect())
    }

    /// One line of a page's timeline as a briefing shows it.
    fn timeline_said(&self, l: &TimelineLine) -> String {
        format!(
            "- {}, {}: {}\n",
            super::world::day_with_year(l.at, self.settings.offset_minutes),
            l.by,
            l.line
        )
    }

    /// The house page while it has no sections: its newest timeline lines, and for the CEO,
    /// who keeps the page, the ask to write what is true now from them (ADR 0122).
    fn house_without_sections(&self, member: &Member, timeline: &[TimelineLine]) -> Vec<Piece> {
        let mut pieces = Vec::new();
        if timeline.is_empty() {
            return vec![Piece::new(
                "world:house",
                "No sections yet.\n",
                Weight::Core,
            )];
        }
        let mut newest: Vec<String> = Vec::new();
        let mut chars = 0;
        for l in timeline.iter().rev() {
            let said = self.timeline_said(l);
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
        pieces
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
        let line = |l: &TimelineLine| self.timeline_said(l);
        if current.is_empty() {
            pieces.extend(self.house_without_sections(member, &timeline));
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

    /// The handover note a member is handed with its task, or that it is new to work others
    /// took turns on.
    fn handover_said(&self, member: &Member, run: &Run) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        if let Some(h) = self.store.latest_handover(task, member.id)? {
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

    /// What changed on the house page since `since`.
    fn changed_since(&self, since: Millis) -> Done<String> {
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
        let day = self.said_day(since);
        if lines.is_empty() {
            return Ok(format!("Nothing on the house page changed since {day}.\n"));
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

    /// The furlough a member is back from, `(from, to)`, if this is its first turn since its
    /// recall.
    pub(super) fn back_from_furlough(
        &self,
        member: &Member,
        run: &Run,
    ) -> Done<Option<(Millis, Millis)>> {
        let (Some(from), Some(to)) = (member.furloughed_at, member.recalled_at) else {
            return Ok(None);
        };
        let since_back = self.store.runs()?.iter().any(|r| {
            r.member == member.id && r.id != run.id && r.started_at.is_some_and(|s| s >= to)
        });
        Ok((!since_back).then_some((from, to)))
    }

    /// The line a briefing opens with for a member back from a furlough.
    pub(super) fn furlough_said(&self, from: Millis, to: Millis) -> String {
        format!(
            "You were furloughed from {} to {}.\n",
            self.said_day(from),
            self.said_day(to)
        )
    }
}
