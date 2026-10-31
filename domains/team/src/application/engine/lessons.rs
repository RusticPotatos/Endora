//! A loop's lessons (ADR 0142, amended 2026-10-31, after snarktank/ralph, whose `progress.txt`
//! keeps "Codebase Patterns" at its top for every pass to read): at most ten short lines per
//! loop, told in each work turn's briefing on the loop right after its item.
//!
//! A line gets in when an item is accepted or sent back: the worker's last word (or the reason
//! it moved its item on with) and the reviewer's notes may carry lines marked
//! [`lessons::MARK`]. The worker's are held for the item until its verdict, so only work a
//! reviewer judged teaches the loop; HQ keeps a line only if it is short, new and holds nothing
//! shaped like a credential ([`lessons::kept`]), and the oldest goes when the list is full. The
//! owner edits the list whole.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::lessons;
use crate::domain::{Member, ProjectId, Refusal, Task, TaskId};

/// Where a loop's lessons are kept, a line each.
fn lessons_key(project: ProjectId) -> String {
    format!("loop_lessons:{}", project.0)
}

/// Where the lessons the item's turns offered wait for its verdict.
fn offered_key(task: TaskId) -> String {
    format!("loop_lessons_offered:{}", task.0)
}

/// Lines as kept: one a line, none blank.
fn lines_of(kept: Option<String>) -> Vec<String> {
    kept.unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// A loop's lessons, oldest first.
    ///
    /// # Errors
    /// A store failure.
    pub fn loop_lessons(&self, project: ProjectId) -> Done<Vec<String>> {
        Ok(lines_of(self.store.setting(&lessons_key(project))?))
    }

    /// The owner sets a loop's lessons, the whole list: each line checked as a lesson is, a
    /// mark taken off, blank lines and repeats left out. The list as kept.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project that does not exist; a line refused, or a list
    /// past ten lines or 1,200 characters, as [`Refusal::Scenario`] with why; a store failure.
    pub fn set_loop_lessons(&self, project: ProjectId, lines: &[String]) -> Done<Vec<String>> {
        let _held = self.held();
        self.project(project)?;
        let list = lessons::owners(lines).map_err(|(line, why)| {
            EngineError::Refused(Refusal::Scenario(format!(
                "{why} (a loop keeps at most {} lessons, {} characters in all): {line}",
                lessons::LOOP_LESSONS_MOST,
                lessons::LOOP_LESSONS_CHARS
            )))
        })?;
        self.store
            .set_setting(&lessons_key(project), &list.join("\n"))?;
        Ok(list)
    }

    /// A turn's words on an item (its last word, or why it moved the item on): the lines marked
    /// as lessons wait for the item's verdict. Nothing outside a loop.
    ///
    /// # Errors
    /// A store failure.
    pub fn offer_lessons(&self, task: TaskId, text: &str) -> Done<()> {
        let said = lessons::marked(text);
        if said.is_empty() {
            return Ok(());
        }
        let _held = self.held();
        if self.store.task(task)?.and_then(|t| t.project).is_none() {
            return Ok(());
        }
        let key = offered_key(task);
        let mut offered = lines_of(self.store.setting(&key)?);
        offered.extend(said);
        let keep_from = offered.len().saturating_sub(lessons::LOOP_LESSONS_MOST);
        self.store
            .set_setting(&key, &offered[keep_from..].join("\n"))?;
        Ok(())
    }

    /// An item judged (accepted or sent back), with the reviewer's `notes`: the lessons its
    /// turns offered, then the reviewer's, go in its loop's list where they may. Held lock.
    pub(super) fn learn_held_lock(&self, task: &Task, notes: &str) -> Done<()> {
        let Some(project) = task.project else {
            return Ok(());
        };
        if self.in_a_scenario(task.id)? {
            return Ok(());
        }
        let key = offered_key(task.id);
        let mut said = lines_of(self.store.setting(&key)?);
        said.extend(lessons::marked(notes));
        if said.is_empty() {
            return Ok(());
        }
        let mut list = self.loop_lessons(project)?;
        for lesson in &said {
            // A line refused (too long, a repeat, a secret) is simply not kept.
            if let Ok(kept) = lessons::kept(&list, lesson) {
                list = kept;
            }
        }
        self.store.set_setting(&key, "")?;
        self.store
            .set_setting(&lessons_key(project), &list.join("\n"))?;
        Ok(())
    }

    /// The loop's lessons as a work turn on one of its items is told them, right after the
    /// item. Members' words among them, so the turn's input is marked as of unknown origin.
    pub(super) fn lessons_briefed(&self, member: &Member, run: &Run, task: &Task) -> Done<String> {
        let Some(project) = task.project else {
            return Ok(String::new());
        };
        if !self.in_the_loops(member.id)? || self.in_a_scenario(task.id)? {
            return Ok(String::new());
        }
        let list = self.loop_lessons(project)?;
        if list.is_empty() {
            return Ok(String::new());
        }
        self.unclassified_message_input(run.id)?;
        let mut told = String::from("Lessons this loop learned, oldest first:\n");
        for lesson in list {
            told.push_str(&format!("- {lesson}\n"));
        }
        Ok(told)
    }
}
