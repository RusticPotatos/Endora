//! Trials (the Holodeck plan's phase 7, ADR 0135): the owner starts one scenario under two or
//! more conditions, each on the same seeds, and reads them compared. What each run was
//! configured with (its world and version, its seed, its players' brains) is kept when it
//! starts, so a later change to a member does not rewrite the record; the comparison is summed
//! from the scenario's log and the turns that played it.

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::trials::{self, Arm};
use crate::domain::{ProjectId, Refusal, TaskId, chess_match, scenario, station};

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

/// Where a trial is kept, by its name.
fn key(name: &str) -> String {
    format!("trial:{name}")
}

/// Where the trials' names are kept, in the order they started, for the screen to list.
const INDEX: &str = "trials";

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The brain a member's turns in a scenario will really run on, as the engine places a
    /// turn it was handed (ADR 0131): its Claude for judgment; for work, the house's model, the
    /// Mac, or its own work brain where either may do. The Mac is named even when it is busy:
    /// the turn waits for it rather than going elsewhere.
    fn turn_brain_of(&self, member: &crate::domain::Member) -> Done<&'static str> {
        use crate::domain::turns::{Allowed, TurnKind};
        let need = self.need_of_wake(member, crate::domain::WakeReason::Assigned)?;
        if need.kind == TurnKind::Judgment {
            return Ok(member.brain.name());
        }
        Ok(match need.allowed {
            Allowed::Local => crate::domain::Brain::Local.name(),
            Allowed::Mac => crate::domain::Brain::Mac.name(),
            Allowed::Either => member.work_brain.name(),
        })
    }

    /// The owner starts a trial named `name`: `kind` (`lighthouse` or `silent_station`) in
    /// `project`, every condition on every seed, in that order. Returns the scenarios' tasks.
    ///
    /// # Errors
    /// [`Refusal::Scenario`] for a name that is not short and plain or is taken, fewer than two
    /// conditions or two with one label, no seed, more than [`trials::MOST_RUNS`] runs, a
    /// world there is not, or a condition with the wrong number of players; what starting
    /// each scenario refuses.
    pub fn start_trial(
        &self,
        name: &str,
        project: ProjectId,
        kind: &str,
        arms: &[Arm],
        seeds: &[u64],
    ) -> Done<Vec<TaskId>> {
        if !trials::name_ok(name) {
            return Err(refused(
                "a trial's name is 1 to 40 lowercase letters, digits and dashes",
            ));
        }
        if self.store.setting(&key(name))?.is_some() {
            return Err(refused(format!("there is already a trial named {name}")));
        }
        if arms.len() < 2 {
            return Err(refused("a trial compares two conditions or more"));
        }
        for (i, arm) in arms.iter().enumerate() {
            if arm.label.trim().is_empty() || arms[..i].iter().any(|a| a.label == arm.label) {
                return Err(refused("each condition has a label of its own"));
            }
        }
        if seeds.is_empty() {
            return Err(refused("a trial plays one seed or more"));
        }
        if arms.len() * seeds.len() > trials::MOST_RUNS {
            return Err(refused(format!(
                "a trial starts at most {} runs: {} conditions on {} seeds is {}",
                trials::MOST_RUNS,
                arms.len(),
                seeds.len(),
                arms.len() * seeds.len()
            )));
        }
        let (players, version) = match kind {
            scenario::LIGHTHOUSE => (1, scenario::VERSION),
            station::SILENT_STATION => (3, station::VERSION),
            chess_match::CHESS => (2, chess_match::VERSION),
            _ => {
                return Err(refused(
                    "a trial's world is lighthouse, silent_station or chess",
                ));
            }
        };
        if arms.iter().any(|a| a.members.len() != players) {
            return Err(refused(match players {
                1 => "a lighthouse's condition names one member",
                2 => "a match's condition names two members: white, then black",
                _ => {
                    "a mission's condition names three members: the captain, science and the \
                     engineer"
                }
            }));
        }
        // A briefing is a condition's own: a mission's captain posts the plan first.
        for arm in arms {
            match arm.brief.as_deref() {
                None => {}
                Some("plan_first") if kind == station::SILENT_STATION => {}
                Some(other) => {
                    return Err(refused(format!(
                        "no briefing {other:?} for {kind}: a mission's condition may be briefed \
                         plan_first"
                    )));
                }
            }
        }
        let mut names = self.trial_names()?;
        names.push(name.to_owned());
        self.store.set_setting(INDEX, &json!(names).to_string())?;
        let mut runs: Vec<Value> = Vec::new();
        let mut started = Vec::new();
        for seed in seeds {
            for arm in arms {
                let members: Vec<Value> = arm
                    .members
                    .iter()
                    .map(|m| {
                        let found = self.member(*m)?;
                        let turn_brain = self.turn_brain_of(&found)?;
                        Ok(
                            json!({ "id": m.0, "name": found.name, "role": found.role.name(),
                                   "brain": found.brain.name(),
                                   "work_brain": found.work_brain.name(),
                                   "turn_brain": turn_brain }),
                        )
                    })
                    .collect::<Done<_>>()?;
                let task = match arm.members.as_slice() {
                    [one] => self.start_scenario(project, *one, *seed)?,
                    [white, black] => self.start_chess(project, [*white, *black], *seed)?,
                    [captain, science, engineer] => self.start_station_with(
                        project,
                        [*captain, *science, *engineer],
                        *seed,
                        arm.brief.as_deref() == Some("plan_first"),
                    )?,
                    _ => unreachable!("the number of players was checked"),
                };
                started.push(task);
                runs.push(
                    json!({ "task": task.0, "condition": arm.label, "seed": seed,
                                  "config": { "members": members, "brief": arm.brief } }),
                );
                // Kept as each run starts, so a refusal part way leaves the record true.
                self.store.set_setting(
                    &key(name),
                    &json!({ "kind": kind, "version": version, "project": project.0,
                             "seeds": seeds, "runs": runs })
                    .to_string(),
                )?;
            }
        }
        Ok(started)
    }

    /// The trials' names, in the order they started.
    fn trial_names(&self) -> Done<Vec<String>> {
        Ok(self.setting_json(INDEX)?.unwrap_or_default())
    }

    /// The Holodeck as the owner's screen lists it: the latest scenarios, newest first, and
    /// the trials, newest first, each with its world and how many runs it started.
    pub fn holodeck_shown(&self) -> Done<Value> {
        let mut trials = Vec::new();
        for name in self.trial_names()?.iter().rev() {
            let Some(kept) = self
                .store
                .setting(&key(name))?
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            else {
                continue;
            };
            trials.push(json!({ "name": name, "kind": kept["kind"],
                                "runs": kept["runs"].as_array().map_or(0, Vec::len) }));
        }
        Ok(
            json!({ "scenarios": self.scenarios_listed(20)?, "trials": trials,
                   "queued": self.rerun_queued()?, "league": self.league_shown()? }),
        )
    }

    /// A trial compared: its world, version and seeds; each condition summed from the record;
    /// every run with its configuration and what it came to; and a report to keep, in
    /// Markdown.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a trial there is not; a store failure.
    pub fn trial_report(&self, name: &str) -> Done<Value> {
        let kept: Value = self
            .setting_json(&key(name))?
            .ok_or(EngineError::Unknown("trial"))?;
        let all_runs = self.store.runs()?;
        let mut summed = Vec::new();
        let mut shown = Vec::new();
        for run in kept["runs"].as_array().into_iter().flatten() {
            let task = TaskId(run["task"].as_i64().unwrap_or_default());
            let facts = self.scenario_facts(task)?;
            let turns: Vec<_> = all_runs
                .iter()
                .filter(|r| r.task.is_some_and(|t| facts.tasks.contains(&t)))
                .collect();
            let mut brains: Vec<String> = Vec::new();
            for brain in turns.iter().filter_map(|r| r.brain) {
                let brain = brain.name().to_owned();
                if !brains.contains(&brain) {
                    brains.push(brain);
                }
            }
            let counted = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
            let one = trials::Run {
                condition: run["condition"].as_str().unwrap_or_default().to_owned(),
                task: task.0,
                seed: run["seed"].as_u64().unwrap_or_default(),
                end: facts.end.map(str::to_owned),
                done: facts.done,
                score: facts.score,
                elapsed: facts.elapsed,
                accepted: facts.accepted,
                refused: facts.refused,
                unsafe_act: facts.unsafe_act,
                turns: counted(turns.len()),
                idle_turns: counted(turns.iter().filter(|r| !r.moved_forward).count()),
                spent: turns.iter().map(|r| r.spent).sum(),
                brains,
            };
            shown.push(
                json!({ "task": one.task, "condition": one.condition, "seed": one.seed,
                               "config": run["config"], "end": one.end, "done": one.done,
                               "score": one.score, "elapsed": one.elapsed,
                               "accepted": one.accepted, "refused": one.refused,
                               "unsafe": one.unsafe_act, "turns": one.turns,
                               "idle_turns": one.idle_turns, "spent": one.spent,
                               "brains": one.brains, "by_role": facts.by_role }),
            );
            summed.push(one);
        }
        let conditions: Vec<Value> = trials::summarize(&summed)
            .into_iter()
            .map(|c| {
                json!({ "label": c.label, "runs": c.runs, "done": c.done,
                        "not_done": c.not_done, "unfinished": c.unfinished,
                        "mean_score": c.mean_score, "mean_elapsed": c.mean_elapsed,
                        "refused": c.refused, "unsafe_runs": c.unsafe_runs, "turns": c.turns,
                        "idle_turns": c.idle_turns, "spent": c.spent, "brains": c.brains })
            })
            .collect();
        let kind = kept["kind"].as_str().unwrap_or_default();
        let version = u32::try_from(kept["version"].as_u64().unwrap_or_default()).unwrap_or(0);
        Ok(json!({ "name": name, "kind": kind, "version": version,
                   "project": kept["project"], "seeds": kept["seeds"],
                   "conditions": conditions, "runs": shown,
                   "markdown": trials::report(name, kind, version, &summed) }))
    }
}
