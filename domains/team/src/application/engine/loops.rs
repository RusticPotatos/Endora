//! One team of loops (ADR 0142): every active project that is not a scenario world is a loop,
//! working its plan (its tasks, by priority then age) one item at a time. A loop with nothing in
//! hand gives its top item to the member at its starting rank, or the nearest one above it who is
//! free to take work. Only members who are one model take part: until the owner sets the
//! members' models, the team works as before.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{
    Brain, Member, MemberId, Millis, ProjectId, ProjectStatus, Refusal, Role, Task, TaskId,
    TaskStatus, WakeReason,
};

/// The five members and their models, ranked (ADR 0142).
const SEATS: [(&str, Brain); 5] = [
    ("Mara", Brain::Opus),
    ("Kit", Brain::Sonnet),
    ("Ada", Brain::Haiku),
    ("Rae", Brain::Mac),
    ("Bo", Brain::Local),
];

/// Where the map's lines are kept: `[{line, by, at, last, uses}]`.
const MAP_KEY: &str = "map_lines";
/// How long a line nobody confirms is kept.
const MAP_FADES_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// The most characters of one line, and of the map in a briefing.
const MAP_LINE_MOST: usize = 190;
const MAP_SAID_MOST: usize = 1_400;
/// The most lines kept; past it, the least confirmed and oldest go.
const MAP_LINES_MOST: usize = 80;
/// How many of a loop's latest lines a turn on it is told.
const NOTES_MOST: usize = 6;

/// The one routine the loops keep: the lab's health check, as Bo's recurring item.
const HEALTH_CHECK: &str = "Check the home lab";

/// How many failed checks at one rank move an item up (ADR 0142).
const FAILURES_A_RANK: u32 = 3;

/// Where an item's failures at its rank are counted.
fn failures_key(task: TaskId) -> String {
    format!("loop_failures:{}", task.0)
}

/// The rank a loop starts at when the owner has not said: the house's model.
const START: u8 = 5;

/// Where a loop's starting rank is kept.
fn rank_key(project: ProjectId) -> String {
    format!("loop_rank:{}", project.0)
}

/// Whether a member takes part in the loops: it is one model, and free to take work.
fn in_the_loops(member: &Member) -> bool {
    member.brain == member.work_brain && member.at_work()
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Whether nothing (a turn queued, started or ended, or a wake) has happened on `task` for
    /// longer than `ms`.
    pub(super) fn idle_for(&self, task: TaskId, ms: Millis) -> Done<bool> {
        let wakes = self.store.wakes()?;
        let last = self
            .store
            .runs_of(task)?
            .iter()
            .flat_map(|r| [Some(r.queued_at), r.started_at, r.ended_at])
            .flatten()
            .chain(wakes.iter().filter(|w| w.task == Some(task)).map(|w| w.at))
            .max()
            .unwrap_or(Millis::MIN);
        Ok(self.clock.now().saturating_sub(last) > ms)
    }

    /// The cut-over to one team of loops (ADR 0142), once, by the owner. Mara, Kit, Ada, Rae
    /// and Bo become the ranked models, plain members with no reporting line; every other
    /// member retires, its open work back on its loop's plan (one playing in a scenario
    /// retires when its game ends: run this again then). Only the lab's health check runs on,
    /// as Bo's. Running it again changes nothing that is done. Returns what it did.
    ///
    /// # Errors
    /// A store failure.
    pub fn become_one_team(&self) -> Done<serde_json::Value> {
        let _held = self.held();
        let now = self.clock.now();
        let members = self.store.members()?;
        let tasks = self.store.tasks()?;
        let (mut seated, mut retired, mut playing) = (Vec::new(), Vec::new(), Vec::new());
        for member in &members {
            if let Some((_, model)) = SEATS
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&member.name))
            {
                self.store.set_model(member.id, *model)?;
                self.store.set_role(member.id, Role::Developer)?;
                self.store.set_reports_to(member.id, None)?;
                seated.push(member.name.clone());
                continue;
            }
            if member.furloughed() {
                continue;
            }
            let held: Vec<&Task> = tasks
                .iter()
                .filter(|t| {
                    !t.status.finished()
                        && (t.owner == Some(member.id) || t.assignee == Some(member.id))
                })
                .collect();
            let mut in_a_game = false;
            for task in &held {
                if self.in_a_scenario(task.id)? {
                    in_a_game = true;
                }
            }
            if in_a_game {
                playing.push(member.name.clone());
                continue;
            }
            for task in held {
                self.store.release(task.id)?;
                self.store.set_assignee(task.id, None)?;
                if task.status == TaskStatus::Doing {
                    self.store.set_task_status(task.id, TaskStatus::Todo)?;
                }
            }
            self.store.furlough_member(member.id, now)?;
            retired.push(member.name.clone());
        }
        let bo = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case("Bo"));
        for mut routine in self.store.routines()? {
            let health = routine.title == HEALTH_CHECK;
            if let (true, Some(bo)) = (health, &bo) {
                routine.assignee = bo.id;
            }
            routine.enabled = health;
            self.store.save_routine(&routine)?;
        }
        Ok(serde_json::json!({ "seated": seated, "retired": retired, "still_playing": playing }))
    }

    /// The owner sets the rank a loop's items start at: 1 (Opus) to 5 (the house's model).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project that does not exist; [`Refusal::Scenario`] for a
    /// rank outside 1 to 5; a store failure.
    pub fn set_loop_rank(&self, project: ProjectId, rank: u8) -> Done<()> {
        let _held = self.held();
        self.project(project)?;
        if !(1..=5).contains(&rank) {
            return Err(EngineError::Refused(Refusal::Scenario(
                "a loop starts at a rank from 1 (Opus) to 5 (the house's model)".to_owned(),
            )));
        }
        self.store
            .set_setting(&rank_key(project), &rank.to_string())?;
        Ok(())
    }

    /// The rank a loop's items start at.
    pub(super) fn loop_rank(&self, project: ProjectId) -> Done<u8> {
        Ok(self
            .store
            .setting(&rank_key(project))?
            .and_then(|r| r.parse().ok())
            .unwrap_or(START))
    }

    /// The members who take part in the loops.
    pub(super) fn loop_members(&self) -> Done<Vec<Member>> {
        Ok(self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect())
    }

    /// The member who holds an item, if it is one who takes part in the loops.
    fn loop_holder(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = task.owner.or(task.assignee) else {
            return Ok(None);
        };
        Ok(self.loop_members()?.into_iter().find(|m| m.id == holder))
    }

    /// The member nearest above `rank` (a smaller number), if any.
    fn one_rank_up(members: &[Member], rank: u8) -> Option<&Member> {
        members
            .iter()
            .filter(|m| m.brain.rank() < rank)
            .max_by_key(|m| (m.brain.rank(), std::cmp::Reverse(m.id.0)))
    }

    /// Who reviews an item in the loops (ADR 0142): the member one rank above its holder, and
    /// the top rank's work the member just below it. `None` for work outside the loops.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) fn loop_reviewer_of(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = self.loop_holder(task)? else {
            return Ok(None);
        };
        let members = self.loop_members()?;
        let rank = holder.brain.rank();
        let up = Self::one_rank_up(&members, rank).or_else(|| {
            members
                .iter()
                .filter(|m| m.id != holder.id && m.brain.rank() > rank)
                .min_by_key(|m| (m.brain.rank(), m.id.0))
        });
        Ok(up.cloned())
    }

    /// Whether an item is held by a member in the loops: its stalls and failures climb the
    /// ranks, not the ladder, and no guard of coordination looks at it.
    pub(super) fn loop_holds(&self, task: &Task) -> Done<bool> {
        Ok(self.loop_holder(task)?.is_some())
    }

    /// Whether a member takes part in the loops.
    pub(crate) fn in_the_loops(&self, member: MemberId) -> Done<bool> {
        Ok(self.loop_members()?.iter().any(|m| m.id == member))
    }

    /// An item that sat with nothing moving it, after its one nudge: one of its failures. Kept
    /// at its rank, its holder is woken to try again; past the third, it has climbed.
    pub(super) fn loop_stalled(&self, task: &Task) -> Done<()> {
        let holder = task.owner.or(task.assignee);
        self.loop_failed(task.id)?;
        let now = self.task(task.id)?;
        if now.owner.or(now.assignee) == holder {
            if let Some(holder) = holder {
                self.wake_held_lock(holder, Some(task.id), WakeReason::Recovery)?;
            }
        }
        Ok(())
    }

    /// A notice made while the loops run is kept and told, but never waits on the owner's desk
    /// (ADR 0142): the desk is for what only the owner decides.
    pub(super) fn quiet_in_the_loops(&self, desk: crate::domain::DeskId) -> Done<()> {
        if !self.loop_members()?.is_empty() {
            self.store.set_desk_status(desk, "seen")?;
        }
        Ok(())
    }

    /// The map's lines still kept at `now`, the most confirmed first, then the latest.
    fn map_lines(&self, now: crate::domain::Millis) -> Done<Vec<serde_json::Value>> {
        let mut lines: Vec<serde_json::Value> = self.setting_json(MAP_KEY)?.unwrap_or_default();
        lines.retain(|l| now - l["last"].as_i64().unwrap_or(0) < MAP_FADES_MS);
        lines.sort_by_key(|l| {
            (
                std::cmp::Reverse(l["uses"].as_i64().unwrap_or(0)),
                std::cmp::Reverse(l["last"].as_i64().unwrap_or(0)),
            )
        });
        Ok(lines)
    }

    /// A member adds a line to the map (ADR 0142): where something lives, found in its turn.
    /// The same line again confirms it. Returns what the member is told.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that is not running; [`EngineError::Unknown`] for an
    /// empty line; a store failure.
    pub fn add_to_map(&self, run: crate::domain::RunId, line: &str) -> Done<String> {
        let _held = self.held();
        let run = self.running(run)?;
        let member = self.member(run.member)?;
        let line = super::one_line(line, MAP_LINE_MOST);
        if line.is_empty() {
            return Err(EngineError::Unknown("map line"));
        }
        let now = self.clock.now();
        let mut lines = self.map_lines(now)?;
        let same = |l: &serde_json::Value| {
            l["line"]
                .as_str()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case(line.trim()))
        };
        let said = if let Some(found) = lines.iter_mut().find(|l| same(l)) {
            found["uses"] = serde_json::json!(found["uses"].as_i64().unwrap_or(0) + 1);
            found["last"] = serde_json::json!(now);
            "It was on the map already: you confirmed it."
        } else {
            lines.push(
                serde_json::json!({ "line": line, "by": member.name, "at": now,
                                           "last": now, "uses": 1 }),
            );
            "It is on the map: every member's briefing shows it."
        };
        lines.truncate(MAP_LINES_MOST);
        self.store
            .set_setting(MAP_KEY, &serde_json::Value::Array(lines).to_string())?;
        Ok(said.to_owned())
    }

    /// The map's lines for the owner: each with who found it, when, and how often confirmed.
    pub(super) fn map_lines_shown(&self) -> Done<serde_json::Value> {
        let lines = self.map_lines(self.clock.now())?;
        Ok(serde_json::json!(
            lines
                .iter()
                .map(
                    |l| serde_json::json!({ "line": l["line"], "by": l["by"], "at": l["at"],
                                              "uses": l["uses"] })
                )
                .collect::<Vec<_>>()
        ))
    }

    /// What a turn in the loops is told besides its task (ADR 0142): the map, most confirmed
    /// first, and what its loop did lately. Both are members' words, so the turn's input is
    /// marked as of unknown origin.
    pub(super) fn loops_said(&self, run: &crate::application::ports::Run) -> Done<String> {
        let mut told = String::new();
        let lines = self.map_lines(self.clock.now())?;
        if !lines.is_empty() {
            self.unclassified_message_input(run.id)?;
            told.push_str(
                "The map, where things are (add_to_map when you find one; the same line again \
                 confirms it):\n",
            );
            for l in &lines {
                let said = format!("- {}\n", l["line"].as_str().unwrap_or(""));
                if told.len() + said.len() > MAP_SAID_MOST {
                    break;
                }
                told.push_str(&said);
            }
        }
        let Some(task) = run.task else {
            return Ok(told);
        };
        let Some(project) = self.task(task)?.project else {
            return Ok(told);
        };
        let names: Vec<String> = self.store.members()?.into_iter().map(|m| m.name).collect();
        let mut lately: Vec<(crate::domain::Millis, String)> = Vec::new();
        for other in self.store.tasks()? {
            if other.project != Some(project) || other.id == task {
                continue;
            }
            for post in self.store.thread(other.id)? {
                if names.contains(&post.author) {
                    lately.push((
                        post.at,
                        format!(
                            "- {} on #{}: {}\n",
                            post.author,
                            other.id.0,
                            super::one_line(&post.text, 200)
                        ),
                    ));
                }
            }
        }
        if lately.is_empty() {
            return Ok(told);
        }
        self.unclassified_message_input(run.id)?;
        lately.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        told.push_str("What this loop did lately, newest first:\n");
        for (_, said) in lately.into_iter().take(NOTES_MOST) {
            told.push_str(&said);
        }
        Ok(told)
    }

    /// An item's failures at its present rank.
    ///
    /// # Errors
    /// A store failure.
    pub fn loop_failures(&self, task: TaskId) -> Done<u32> {
        Ok(self
            .store
            .setting(&failures_key(task))?
            .and_then(|n| n.parse().ok())
            .unwrap_or(0))
    }

    /// One of an item's failures (a failed check, or a send-back), the lock held. The third at
    /// a rank hands it to the member one rank up; the top rank's third goes to the owner.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) fn loop_failed(&self, task: TaskId) -> Done<()> {
        let found = self.task(task)?;
        let Some(holder) = self.loop_holder(&found)? else {
            return Ok(());
        };
        let failures = self.loop_failures(task)? + 1;
        if failures < FAILURES_A_RANK {
            self.store
                .set_setting(&failures_key(task), &failures.to_string())?;
            return self.post_held_lock(
                super::Author::Endora,
                task,
                &format!(
                    "Failure {failures} of {FAILURES_A_RANK} at {}'s rank.",
                    holder.name
                ),
            );
        }
        self.store.set_setting(&failures_key(task), "0")?;
        let members = self.loop_members()?;
        let Some(up) = Self::one_rank_up(&members, holder.brain.rank()) else {
            let text = format!(
                "{} failed at every rank, Mara's last: {}. Its thread says what was tried.",
                found.title, holder.name
            );
            self.store
                .add_desk("question", Some(task), None, None, &text, self.clock.now())?;
            return self.emit(
                crate::domain::EventKind::DeskAdded,
                serde_json::json!({ "kind": "question", "task": task.0 }),
            );
        };
        let up: MemberId = up.id;
        let to = self.member(up)?;
        self.store.release(task)?;
        self.store.set_assignee(task, Some(up))?;
        if found.status != TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Todo)?;
        }
        self.post_held_lock(
            super::Author::Endora,
            task,
            &format!(
                "It failed {FAILURES_A_RANK} times at {}'s rank: it goes to {}, one rank up.",
                holder.name, to.name
            ),
        )?;
        self.wake_held_lock(up, Some(task), WakeReason::Assigned)?;
        Ok(())
    }

    /// The loops' part of the tick: each loop with nothing in hand hands out its top item.
    pub(super) fn loops_tick(&self) -> Done<()> {
        let members: Vec<Member> = self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        let tasks = self.store.tasks()?;
        for project in self.store.projects()? {
            if project.status != ProjectStatus::Active || project.pause.is_some() {
                continue;
            }
            if self.holds_scenarios(project.id).is_ok() {
                continue;
            }
            let plan: Vec<_> = tasks
                .iter()
                .filter(|t| t.project == Some(project.id))
                .collect();
            let in_hand = plan.iter().any(|t| {
                (t.assignee.is_some() || t.owner.is_some())
                    && matches!(t.status, TaskStatus::Todo | TaskStatus::Doing)
            });
            if in_hand {
                continue;
            }
            let Some(top) = plan
                .iter()
                .filter(|t| {
                    t.status == TaskStatus::Todo && t.assignee.is_none() && t.owner.is_none()
                })
                .min_by_key(|t| (t.priority, t.id.0))
            else {
                continue;
            };
            let start = self.loop_rank(project.id)?;
            // The member at the starting rank, or the nearest one above it.
            let Some(member) = members
                .iter()
                .filter(|m| m.brain.rank() <= start)
                .max_by_key(|m| (m.brain.rank(), std::cmp::Reverse(m.id.0)))
            else {
                continue;
            };
            self.store.set_assignee(top.id, Some(member.id))?;
            self.post_held_lock(
                super::Author::Endora,
                top.id,
                &format!(
                    "{} takes this: the next item of the {} loop.",
                    member.name, project.title
                ),
            )?;
            self.wake_held_lock(member.id, Some(top.id), WakeReason::Assigned)?;
        }
        Ok(())
    }
}
