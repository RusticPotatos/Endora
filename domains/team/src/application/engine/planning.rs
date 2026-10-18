//! The CEO's weekly planning (ADR 0113, rebuilt by ADR 0120): a routine of its own, apart from
//! the daily review, whose briefing is for steering: each goal's strategy, each area's
//! measures over the week, the proposals waiting, the budget split across the projects, and
//! the goals with no project. The repeats (tasks by the containers they named, the alarms that
//! came back) are found here too, but shown to each area's lead in its weekly patterns turn:
//! the leads propose, the CEO decides. Lean on purpose: the CEO thinks on its most expensive
//! model.

use crate::application::ports::{Clock, NewRoutine, TeamStore, WorkerAdapter};
use crate::domain::review;
use crate::domain::routines::DAY_MS;
use crate::domain::{GoalId, GoalStatus, ProjectStatus, Role, RoutineId, TaskId};

use super::{Done, Engine};

/// What the CEO's weekly planning asks of it (ADR 0120: the goal of the goal, the plan of the
/// plan; the leads spot the patterns and propose).
pub const PLANNING: &str = "Plan the house once a week, from the strategy and the measures: \
    read your briefing (each goal's strategy, each area's measures over the week, the \
    proposals waiting, and the budget split across the projects). Decide each proposal with \
    decide_project (yes, no, or not now), and recommend a project's cap there when its money \
    should change. Rewrite a goal's strategy with set_strategy where a bet changed or a \
    measure says it should. Propose a goal the house needs with propose_goal (it goes to the \
    board). Where nothing is new, change nothing. Then close this as done, saying what you \
    decided.";

/// Where HQ keeps which routine is the CEO's planning, once it made it.
const PLANNING_ROUTINE: &str = "planning_routine";

/// The most groups, or alarms that came back, the planning lists.
const MOST_PATTERNS: usize = 10;

/// The most task numbers or event ids a line gives.
const MOST_IDS: usize = 8;

fn ids<T: core::fmt::Display>(prefix: &str, all: &[T]) -> String {
    let shown: Vec<String> = all
        .iter()
        .take(MOST_IDS)
        .map(|i| format!("{prefix}{i}"))
        .collect();
    let more = all.len().saturating_sub(MOST_IDS);
    if more == 0 {
        return shown.join(", ");
    }
    format!("{} and {more} more", shown.join(", "))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Makes the CEO's weekly planning, once: a routine "Planning" for the CEO under `goal`,
    /// every 7 days, apart from its daily review. Nothing with no CEO, or once made (the owner
    /// may switch it off like any routine). Returns the routine, if it made one.
    ///
    /// # Errors
    /// What [`Self::add_routine`] refuses; a store failure.
    pub fn seed_planning(&self, goal: GoalId) -> Done<Option<RoutineId>> {
        if self.store.setting(PLANNING_ROUTINE)?.is_some() {
            return Ok(None);
        }
        let Some(ceo) = self
            .store
            .members()?
            .into_iter()
            .filter(|m| m.role == Role::Ceo)
            .min_by_key(|m| m.id)
        else {
            return Ok(None);
        };
        let made = self.add_routine(
            super::Author::Endora,
            NewRoutine {
                title: "Planning".to_owned(),
                description: PLANNING.to_owned(),
                goal,
                project: None,
                assignee: ceo.id,
                every_ms: 7 * DAY_MS,
                on_lab_trouble: false,
            },
        )?;
        self.store
            .set_setting(PLANNING_ROUTINE, &made.0.to_string())?;
        Ok(Some(made))
    }

    /// Whether `task` is the CEO's planning's open task.
    pub(super) fn is_planning(&self, task: TaskId) -> Done<bool> {
        let Some(planning) = self.store.setting(PLANNING_ROUTINE)? else {
            return Ok(false);
        };
        Ok(self
            .routine_of(task)?
            .is_some_and(|r| r.id.0.to_string() == planning))
    }

    /// The planning's picture, after the CEO's opening lines (ADR 0120): strategy, the week's
    /// measures, the proposals and the budget split. The repeats in each area are its lead's
    /// to find, in its own weekly patterns turn.
    pub(super) fn planning_picture(&self) -> Done<String> {
        let mut told = String::from(
            "This is your weekly planning: steer by the strategy and the measures; decide the \
             proposals waiting; change nothing where nothing is new.\n",
        );
        told.push_str(&self.strategies_said(None)?);
        told.push_str(&self.areas_said(7)?);
        told.push_str(&self.proposals_said()?);
        told.push_str(&self.budget_said()?);
        let bare = self.goals_without_projects()?;
        if !bare.is_empty() {
            told.push_str(&format!("Goals with no project: {}.\n", bare.join("; ")));
        }
        Ok(told)
    }

    /// The open goals that have no active or paused project.
    fn goals_without_projects(&self) -> Done<Vec<String>> {
        let projects = self.store.projects()?;
        Ok(self
            .store
            .goals()?
            .into_iter()
            .filter(|g| g.status == GoalStatus::Active)
            .filter(|g| {
                !projects.iter().any(|p| {
                    p.goal == g.id
                        && matches!(p.status, ProjectStatus::Active | ProjectStatus::Paused)
                })
            })
            .map(|g| format!("#{} {}", g.id, g.title))
            .collect())
    }

    /// The last 30 days' tasks grouped by the containers they named, with the alarms about
    /// the same containers; and the alarms that came back.
    pub(super) fn patterns(
        &self,
        made: &[TaskId],
        alarms: &[(i64, &str)],
        lab: &[String],
    ) -> Done<String> {
        let mut told = String::from("The last 30 days' tasks, by the containers they named:\n");
        let mut groups: Vec<(Vec<String>, Vec<TaskId>)> = Vec::new();
        for id in made {
            let Some(task) = self.store.task(*id)? else {
                continue;
            };
            let names = review::about(&task.title, &task.description, lab);
            if names.is_empty() {
                continue;
            }
            match groups.iter_mut().find(|(n, _)| *n == names) {
                Some((_, tasks)) => tasks.push(task.id),
                None => groups.push((names, vec![task.id])),
            }
        }
        groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        if groups.is_empty() {
            told.push_str("- none named a container\n");
        }
        let about: Vec<(i64, Vec<String>)> = alarms
            .iter()
            .map(|(id, said)| (*id, review::about(said, "", lab)))
            .collect();
        for (names, tasks) in groups.iter().take(MOST_PATTERNS) {
            let raised = about
                .iter()
                .filter(|(_, n)| n.iter().any(|x| names.contains(x)))
                .count();
            told.push_str(&format!(
                "- {}: {} task{} ({}), {raised} alarm{}\n",
                names.join(", "),
                tasks.len(),
                if tasks.len() == 1 { "" } else { "s" },
                ids("#", tasks),
                if raised == 1 { "" } else { "s" }
            ));
        }
        told.push_str("Alarms that came back in the last 30 days:\n");
        let mut again: Vec<(Vec<String>, Vec<i64>)> = Vec::new();
        for (id, names) in about {
            if names.is_empty() {
                continue;
            }
            match again.iter_mut().find(|(n, _)| *n == names) {
                Some((_, events)) => events.push(id),
                None => again.push((names, vec![id])),
            }
        }
        again.retain(|(_, events)| events.len() > 1);
        again.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        if again.is_empty() {
            told.push_str("- none came back\n");
        }
        for (names, events) in again.iter().take(MOST_PATTERNS) {
            told.push_str(&format!(
                "- {}: {} alarms (events {})\n",
                names.join(", "),
                events.len(),
                ids("", events)
            ));
        }
        Ok(told)
    }
}
