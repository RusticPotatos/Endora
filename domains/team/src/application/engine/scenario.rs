//! Scenarios (the Holodeck plan's phase 5): the owner starts one as a task in a project that
//! gives its work the scenario tool alone; the member acts through that tool; the world, in
//! code, says what each action did and closes the task when the lamp is lit or time is out.
//! The state is never kept: it is the seed's lighthouse with the log's accepted actions
//! replayed, and the log is append-only, so a reset is a new task with the same seed.

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, NewTask, ScenarioAction, TeamStore, WorkerAdapter};
use crate::domain::closure::{Closure, DoneWhy, DropWhy};
use crate::domain::scenario::{self, Action, End, Lighthouse};
use crate::domain::{EventKind, MemberId, Priority, ProjectId, Refusal, RunId, TaskId, TaskStatus};

/// The tool's name, which a scenario's project lists alone.
pub const TOOL: &str = "scenario";

/// The longest action, as written, the log keeps.
const ACTION_MOST: usize = 60;

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

/// Where a scenario's seed is kept, by its task.
fn key(task: TaskId) -> String {
    format!("scenario:{}", task.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The owner starts the lighthouse for `member`, from `seed`, as a task in `project`, which
    /// must give its work the scenario tool and nothing else. Starting it again with the same
    /// seed is its reset: a new task, the same world.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project there is not; [`Refusal::Scenario`] for a project
    /// that does not list the scenario tool alone; what adding the task refuses.
    pub fn start_scenario(&self, project: ProjectId, member: MemberId, seed: u64) -> Done<TaskId> {
        let _held = self.held();
        let _asked = self.for_owner();
        let found = self.project(project)?;
        if self.project_tools(project)?.as_deref() != Some(&[TOOL.to_owned()][..]) {
            return Err(refused(format!(
                "a scenario's project gives its work the {TOOL} tool and nothing else: set \
                 project \"{}\"'s tools to [\"{TOOL}\"] first",
                found.title
            )));
        }
        let task = self.add_task_held_lock(
            Author::Owner,
            NewTask {
                goal: found.goal,
                project: Some(project),
                parent: None,
                title: format!("The dark lighthouse (seed {seed})"),
                description: scenario::lighthouse_brief(),
                assignee: Some(member),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )?;
        self.store.set_setting(&key(task), &seed.to_string())?;
        Ok(task)
    }

    /// A task's scenario, by its seed, if it is one.
    ///
    /// # Errors
    /// A store failure.
    pub fn scenario_of(&self, task: TaskId) -> Done<Option<u64>> {
        Ok(self.store.setting(&key(task))?.and_then(|s| s.parse().ok()))
    }

    /// A scenario as its log replays: the seed's lighthouse with the accepted actions applied
    /// in order.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no scenario; [`Refusal::Scenario`] for a log
    /// that does not replay, which an append-only log written by this engine never is.
    pub fn replay_scenario(&self, task: TaskId) -> Done<Lighthouse> {
        let seed = self
            .scenario_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let actions: Vec<Action> = self
            .store
            .scenario_actions(task)?
            .iter()
            .filter(|a| a.ok)
            .filter_map(|a| Action::read(&a.action))
            .collect();
        Lighthouse::replay(seed, &actions).map_err(refused)
    }

    /// A scenario as the owner's screen shows it: its seed, where it stands, how it ended and,
    /// once it has, why the lamp was dark; and its log step by step, refused actions too, each
    /// with who acted and what the world said.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no scenario; a store failure.
    pub fn scenario_shown(&self, task: TaskId) -> Done<Value> {
        let seed = self
            .scenario_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let now = self.replay_scenario(task)?;
        let names = self.names()?;
        let log: Vec<Value> = self
            .store
            .scenario_actions(task)?
            .iter()
            .map(|a| {
                json!({ "action": a.action, "ok": a.ok, "said": a.said, "run": a.run.0,
                        "member": a.member.0, "by": names.get(&a.member), "at": a.at })
            })
            .collect();
        let cause = now.end().map(|_| match now.cause {
            scenario::Cause::Bulb => "bulb",
            scenario::Cause::Fuse => "fuse",
        });
        Ok(
            json!({ "task": task.0, "kind": scenario::LIGHTHOUSE, "seed": seed,
                   "minutes": now.minutes, "limit": scenario::TIME_LIMIT, "lit": now.lit,
                   "end": now.end().map(End::name), "cause": cause, "said": now.look(),
                   "log": log }),
        )
    }

    /// Whether a turn's work is a scenario: the scenario tool is offered in it.
    pub(crate) fn in_scenario(&self, run: RunId) -> Done<bool> {
        let Some(task) = self.store.run(run)?.and_then(|r| r.task) else {
            return Ok(false);
        };
        Ok(self.scenario_of(task)?.is_some())
    }

    /// Why a turn is refused one of HQ's tools on a scenario, if it is: moving a scenario's
    /// task is the world's, never a member's word.
    ///
    /// # Errors
    /// A store failure.
    pub fn scenario_refuses(&self, tool: &str, args: &Value) -> Done<Option<String>> {
        if tool != "set_status" {
            return Ok(None);
        }
        let Some(task) = args["task"].as_i64() else {
            return Ok(None);
        };
        if self.scenario_of(TaskId(task))?.is_none() {
            return Ok(None);
        }
        Ok(Some(
            "a scenario is closed by the world, not by anyone's word: act in it with the \
             scenario tool, and it closes itself when the lamp is lit or time is out"
                .to_owned(),
        ))
    }

    /// `scenario`: `look` says where things stand; any other action is the world's to accept
    /// or refuse. Every action is kept in the log, refused ones too, and said as an event; an
    /// accepted one that ends the scenario closes its task.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::Scenario`] for a turn whose task is no scenario, an
    /// action the world does not know, or one it refuses; a store failure.
    pub fn scenario_in(&self, run: RunId, action: &str, part: Option<&str>) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let no_scenario = || refused("this turn's task is no scenario: there is nothing to act in");
        let task = turn.task.ok_or_else(no_scenario)?;
        if self.scenario_of(task)?.is_none() {
            return Err(no_scenario());
        }
        let now = self.replay_scenario(task)?;
        if action.trim() == "look" {
            return Ok(now.look());
        }
        let written = match part {
            Some(part) if action.trim() == "replace_part" => {
                format!("replace_part {}", part.trim())
            }
            _ => action.trim().to_owned(),
        };
        let stepped = match Action::read(&written) {
            Some(act) => now.step(act),
            None => Err(
                "`action` is look, inspect, test_circuit or replace_part (with part: bulb or fuse)"
                    .to_owned(),
            ),
        };
        let kept: String = written.chars().take(ACTION_MOST).collect();
        let (next, said) = match stepped {
            Ok(done) => done,
            Err(why) => {
                self.act_kept(task, run, turn.member, &kept, false, &why, &now)?;
                return Err(refused(why));
            }
        };
        self.act_kept(task, run, turn.member, &kept, true, &said, &next)?;
        if let Some(mut found) = self.store.run(run)? {
            found.moved_forward = true;
            self.store.save_run(&found)?;
        }
        if let Some(end) = next.end() {
            self.scenario_ended(task, end, &said)?;
        }
        Ok(said)
    }

    /// One action into the log, and its event.
    #[allow(clippy::too_many_arguments)]
    fn act_kept(
        &self,
        task: TaskId,
        run: RunId,
        member: MemberId,
        action: &str,
        ok: bool,
        said: &str,
        after: &Lighthouse,
    ) -> Done<()> {
        self.store.add_scenario_action(&ScenarioAction {
            id: 0,
            task,
            run,
            member,
            action: action.to_owned(),
            ok,
            said: said.to_owned(),
            at: self.clock.now(),
        })?;
        let by = self.member(member)?.name;
        self.emit(
            EventKind::ScenarioActed,
            json!({ "task": task.0, "run": run.0, "member": member.0, "by": by,
                    "action": action, "ok": ok, "said": said, "minutes": after.minutes,
                    "end": after.end().map(End::name) }),
        )
    }

    /// The world closes its task: done when solved, dropped (it cannot go on) when failed.
    fn scenario_ended(&self, task: TaskId, end: End, said: &str) -> Done<()> {
        let current = self.task(task)?;
        let (status, closure) = match end {
            End::Solved => (TaskStatus::Done, Closure::Done(DoneWhy::MetCriteria)),
            End::Failed => (
                TaskStatus::Dropped,
                Closure::Dropped(DropWhy::BlockedForever),
            ),
        };
        self.store.set_task_status(task, status)?;
        self.emit_status(&current, status)?;
        self.closed(task, closure, None, "endora")?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!("The scenario is {}: {said}", end.name()),
        )?;
        self.after_finishing(&current)
    }
}
