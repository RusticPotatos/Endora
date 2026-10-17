//! The CEO's weekly planning (ADR 0113): a routine of its own, apart from the daily review,
//! whose briefing is for proposing from patterns. It shows, per goal, its projects and their
//! money; the last 30 days' tasks grouped by the home-lab containers they named; the alarms
//! that came back; the proposals waiting; and the goals with no project. Each line gives the
//! task numbers and event ids a proposal may cite as its evidence. Lean on purpose: the CEO
//! thinks on its most expensive model.

use super::{Done, Engine, alarm_in};
use crate::application::ports::{Clock, NewRoutine, TeamStore, WorkerAdapter};
use crate::domain::projects::dollars;
use crate::domain::review;
use crate::domain::routines::DAY_MS;
use crate::domain::{GoalId, GoalStatus, ProjectStatus, Role, RoutineId, TaskId};

/// What the CEO's weekly planning asks of it.
pub const PLANNING: &str = "Plan the house's work, once a week, from what keeps coming back: \
    read your briefing's patterns (the projects and their money, the tasks by the containers \
    they named, the alarms that came back, the proposals waiting, the goals with no project). \
    Decide each proposal waiting with decide_project. Propose a project for work that keeps \
    coming back with propose_project, and a goal the house needs with propose_goal (it goes to \
    the board), each citing the task numbers and event ids it rests on. Where nothing is new, \
    change nothing. Then close this as done, saying what you decided.";

/// Where HQ keeps which routine is the CEO's planning, once it made it.
const PLANNING_ROUTINE: &str = "planning_routine";

/// How far back the planning looks.
const PLANNING_LOOKS_BACK_MS: i64 = 30 * DAY_MS;

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

    /// The planning's picture, after the CEO's opening lines.
    pub(super) fn planning_picture(&self) -> Done<String> {
        let mut told = String::from(
            "This is your weekly planning: propose from patterns, citing their task numbers and \
             event ids as evidence; decide the proposals waiting; change nothing where nothing \
             is new.\n",
        );
        let members = self.store.members()?;
        let projects = self.store.projects()?;
        let name_of = |id| {
            members
                .iter()
                .find(|m| m.id == id)
                .map_or("nobody", |m| m.name.as_str())
        };
        told.push_str("Goals and their projects:\n");
        let mut bare = Vec::new();
        for goal in self.store.goals()? {
            if !matches!(goal.status, GoalStatus::Active | GoalStatus::Proposed) {
                continue;
            }
            told.push_str(&format!(
                "- #{} {} ({})\n",
                goal.id,
                goal.title,
                goal.status.name()
            ));
            let mine: Vec<_> = projects
                .iter()
                .filter(|p| {
                    p.goal == goal.id
                        && matches!(p.status, ProjectStatus::Active | ProjectStatus::Paused)
                })
                .collect();
            if mine.is_empty() {
                bare.push(format!("#{} {}", goal.id, goal.title));
            }
            for project in mine {
                told.push_str(&format!("  - {}\n", self.project_line(project, &members)?));
            }
        }
        let now = self.clock.now();
        let since = now - PLANNING_LOOKS_BACK_MS;
        let events = self.store.events_after(0)?;
        let recent: Vec<_> = events.iter().filter(|e| e.at >= since).collect();
        let made: Vec<TaskId> = recent
            .iter()
            .filter(|e| e.kind == "task.created")
            .filter_map(|e| e.data["task"].as_i64().map(TaskId))
            .collect();
        let alarms: Vec<(i64, &str)> = recent
            .iter()
            .filter_map(|e| Some((e.id, alarm_in(e)?)))
            .collect();
        match self.lab_names() {
            None => told.push_str(
                "The last 30 days' tasks by container: the node has not said yet what \
                 containers the home lab has.\n",
            ),
            Some(lab) => told.push_str(&self.patterns(&made, &alarms, &lab)?),
        }
        told.push_str("Project proposals waiting for your decision:\n");
        let waiting: Vec<_> = projects
            .iter()
            .filter(|p| p.status == ProjectStatus::Proposed)
            .collect();
        if waiting.is_empty() {
            told.push_str("- none\n");
        }
        for p in waiting {
            let asks = p
                .asked_cap
                .map(|c| format!(", asks {} a month", dollars(c)))
                .unwrap_or_default();
            let evidence: Vec<String> = p.evidence.iter().map(|e| e.said()).collect();
            told.push_str(&format!(
                "- #{} {} (by {}, for goal #{}, led by {}{asks}): {} Evidence: {}.\n",
                p.id,
                p.title,
                p.created_by,
                p.goal,
                name_of(p.lead),
                p.reason,
                evidence.join(", ")
            ));
        }
        if !bare.is_empty() {
            told.push_str(&format!("Goals with no project: {}.\n", bare.join("; ")));
        }
        Ok(told)
    }

    /// The last 30 days' tasks grouped by the containers they named, with the alarms about
    /// the same containers; and the alarms that came back.
    fn patterns(&self, made: &[TaskId], alarms: &[(i64, &str)], lab: &[String]) -> Done<String> {
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
