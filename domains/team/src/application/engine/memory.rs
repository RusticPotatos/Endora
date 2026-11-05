//! What a turn is told of the team's world (ADR 0122): its situation, decided from the record,
//! and the pieces that situation needs, composed within the member's budget. Whatever is left
//! out is named at the end of the briefing, and the run records the situation and what it was
//! told.

use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::relevance::Relevance;
use crate::domain::situations::{self, Briefed, Composed, Facts, Piece, Situation, Weight};
use crate::domain::world::{HOUSE, Section, Stage};
use crate::domain::{Member, Millis, ProjectId, RunId};

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
                t.holder() == Some(member.id)
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
    fn inherit_briefed(&self, run: &Run, briefed: &[&String]) -> Done<()> {
        if briefed.is_empty() {
            return Ok(());
        }
        let sections = self.store.sections()?;
        for name in briefed {
            match Self::piece_written_in(name, &sections) {
                Some(source) => self.inherit_input_from(run.id, Some(source))?,
                None => self.unclassified_message_input(run.id)?,
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

    /// The turn that wrote a briefed piece: a section's own; none known for any other piece.
    fn piece_written_in(name: &str, sections: &[Section]) -> Option<RunId> {
        sections
            .iter()
            .find(|s| s.address() == name)
            .and_then(|s| s.written_in_run)
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
                        "map" | "world:lab" | "world:house"
                    )
            })
            .map(|piece| piece.name.clone())
            .collect();
        let composed = situations::compose(situations::budget(member.brain, situation), pieces);
        // Each piece briefed is classified by the turns that wrote it (ADR 0138 A2d): a house
        // section, a story and the house's timeline lines by their recorded runs. What has none,
        // and every other piece, fails closed.
        let briefed: Vec<&String> = shown
            .iter()
            .filter(|name| composed.briefed.contains(name))
            .collect();
        self.inherit_briefed(run, &briefed)?;
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

    /// What a turn's item names, to weigh the world's lines against (ADR 0142, amended
    /// 2026-10-30): its title, description and criteria, and its project's title. `None` for a
    /// turn about no task, or with none to brief (downtime), which are told as before.
    pub(super) fn item_relevance(&self, run: &Run) -> Done<Option<Relevance>> {
        let Some(task) = run.task else {
            return Ok(None);
        };
        if self.reason_for(run)? == crate::domain::WakeReason::Downtime.name() {
            return Ok(None);
        }
        let task = self.task(task)?;
        let project = match task.project {
            Some(id) => self
                .store
                .projects()?
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.title),
            None => None,
        };
        let mut texts: Vec<&str> = vec![&task.title, &task.description];
        texts.extend(task.criteria.iter().map(|c| c.text.as_str()));
        texts.extend(project.as_deref());
        Ok(Some(Relevance::of_item(&texts)))
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
        // What changed while it was away: since its last turn.
        if situation == Situation::AfterGap {
            let since = self.last_turn_of(member, run)?.unwrap_or(0);
            pieces.push(Piece::new(
                "what changed",
                self.changed_since(since)?,
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
        let relevance = self.item_relevance(run)?;
        pieces.extend(self.house_pieces(situation, relevance.as_ref())?);
        // An item in a repository outside the lab is told that one, not how the lab is built
        // (ADR 0115, amended 2026-11-05: the lab's rules hold in its fork alone).
        if let Some(repo) = match run.task {
            Some(task) => self.outside_repo_of_task(task)?,
            None => None,
        } {
            pieces.push(Piece::new(
                "repository",
                crate::domain::wing::outside_repo_said(&repo),
                Weight::Core,
            ));
            return Ok(pieces);
        }
        // How the lab is built, for every member (ADR 0142); on an item, its lines about it.
        let lab = match &relevance {
            Some(relevance) => self.lab_said_about(relevance),
            None => self.lab_said(),
        };
        pieces.push(Piece::new("world:lab", lab, Weight::Recommended));
        Ok(pieces)
    }

    /// The house page's pieces for a member: what the owner wrote and its canon, and in a fresh
    /// start its insights too. On an item (`relevance`), only the sections that name what it
    /// names, whatever their stage short of an observation, and where the rest is read.
    fn house_pieces(
        &self,
        situation: Situation,
        relevance: Option<&Relevance>,
    ) -> Done<Vec<Piece>> {
        let mut pieces = Vec::new();
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
        let header = "The house page's canon (world:house):\n";
        let shown: Vec<&(Section, Stage, String)> = current
            .iter()
            .filter(|(s, stage, _)| {
                s.written_by == "the owner"
                    || *stage == Stage::Canon
                    || (situation == Situation::Fresh && *stage == Stage::Insight)
                    || (relevance.is_some() && *stage == Stage::Insight)
            })
            .collect();
        // On an item, the sections that name what it names, by what a briefing shows of them,
        // and one line for the rest.
        let (shown, left) = match relevance {
            Some(r) => r.keep(shown, |(s, _, _)| format!("{} {}", s.heading, s.headline)),
            None => (shown, 0),
        };
        if !shown.is_empty() {
            let header = if relevance.is_some() {
                "The house page (world:house), its sections about this item:\n"
            } else {
                header
            };
            pieces.push(Piece::new("world:house", header, Weight::Core));
        }
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
        if left > 0 {
            pieces.push(Piece::new(
                "world:house",
                format!(
                    "The house page has {left} more sections, not about this item: read_world \
                     world:house.\n"
                ),
                Weight::Core,
            ));
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
}
