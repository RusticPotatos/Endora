//! Scenarios (the Holodeck plan's phases 5 and 6, ADR 0135): the owner starts one as a task in
//! a project that gives its work the scenario tool alone; members act through that tool; the
//! world, in code, says what each action did and closes its tasks when it ends. The state is
//! never kept: it is the seed's world with the log's accepted actions replayed, and the log is
//! append-only, so a reset is a new task with the same seed.
//!
//! Three worlds. The lighthouse is one member's task. Silent Station is a crew's: the captain
//! holds the mission, science and the engineer a part of it each, all acting in the mission's
//! one world and log, each as its role alone; an accepted action wakes the rest of the crew.
//! A chess match is two members': white holds the match and black a part of it, a move a turn,
//! the opponent woken when it is its move.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, NewTask, ScenarioAction, TeamStore, WorkerAdapter};
use crate::domain::chess_match::{self, Match, Side};
use crate::domain::closure::{Closure, DoneWhy, DropWhy};
use crate::domain::scenario::{self, Lighthouse};
use crate::domain::station::{self, Station};
use crate::domain::{
    EventKind, MemberId, Priority, ProjectId, Refusal, RunId, TaskId, TaskStatus, WakeReason,
};

/// The tool's name, which a scenario's project lists alone.
pub const TOOL: &str = "scenario";

/// The longest action, as written, the log keeps.
const ACTION_MOST: usize = 400;

/// A mission's mark word where nothing is done before the captain's plan (a trial's briefing).
const PLAN_FIRST: &str = "silent_station_plan";

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

/// Where a task's place in a scenario is kept: `<seed>` for a lighthouse;
/// `silent_station:<seed>:<science's task>:<engineer's task>` for a mission;
/// `chess:<seed>:<black's task>` for a match; `crew:<mission>:<role>` for a part of either.
fn key(task: TaskId) -> String {
    format!("scenario:{}", task.0)
}

/// How long a world whose players have no turn coming sits before they are woken again.
const IDLE_MS: i64 = 10 * 60 * 1000;

/// How many times its players are woken again with nothing done before the world is abandoned.
const REWAKES: i64 = 3;

/// Where a world's idleness is kept, by its task.
fn idle_key(world: TaskId) -> String {
    format!("scenario_idle:{}", world.0)
}

/// Where an earlier round's label is kept, by its world's task (the owner's re-run).
fn round_key(world: TaskId) -> String {
    format!("scenario_round:{}", world.0)
}

/// The scenarios queued to run again, oldest first: `kind:seed:project:plan:players`, `;`
/// between them.
const RERUN_QUEUE: &str = "holodeck_rerun";

/// Which world a scenario is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The lighthouse (phase 5).
    Lighthouse,
    /// Silent Station (phase 6).
    Station,
    /// A chess match.
    Chess,
}

impl Kind {
    /// Its name.
    const fn name(self) -> &'static str {
        match self {
            Self::Lighthouse => scenario::LIGHTHOUSE,
            Self::Station => station::SILENT_STATION,
            Self::Chess => chess_match::CHESS,
        }
    }

    /// The role of the member who holds the world's own task: a mission's captain, a match's
    /// white.
    const fn holder(self) -> Option<&'static str> {
        match self {
            Self::Lighthouse => None,
            Self::Station => Some(station::Role::Captain.name()),
            Self::Chess => Some(Side::White.name()),
        }
    }

    /// A role's name in this world, as its own.
    fn role(self, name: &str) -> Option<&'static str> {
        match self {
            Self::Lighthouse => None,
            Self::Station => station::Role::from_name(name).map(station::Role::name),
            Self::Chess => Side::from_name(name).map(Side::name),
        }
    }
}

/// Where a task stands in a scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    /// The task whose world and log it is: the lighthouse's own, or the mission.
    world: TaskId,
    kind: Kind,
    seed: u64,
    /// The world's parts: a mission's science's then engineer's, a match's black's.
    parts: Vec<TaskId>,
    /// The role this task's member wears: in a mission, or a side in a match.
    role: Option<&'static str>,
    /// Whether nothing is done before the captain posts the crew's plan.
    plan_first: bool,
}

/// What the record says of one scenario: what a trial sums.
pub(super) struct Facts {
    /// How it ended, if it has.
    pub end: Option<&'static str>,
    /// Whether that end is the work done.
    pub done: bool,
    /// What it scored.
    pub score: u32,
    /// Time spent in the world.
    pub elapsed: u32,
    /// Actions the world accepted.
    pub accepted: u32,
    /// Actions it refused.
    pub refused: u32,
    /// Whether anyone was hurt.
    pub unsafe_act: bool,
    /// The tasks its turns were taken on: the world's, and its parts.
    pub tasks: Vec<TaskId>,
    /// Each role's actions, accepted and refused, with the member who wore it: a match's
    /// illegal moves by side.
    pub by_role: Vec<Value>,
}

/// A world as it stands, whichever it is.
enum World {
    Lighthouse(Lighthouse),
    Station(Station),
    Chess(Match),
}

impl World {
    fn replay(kind: Kind, seed: u64, plan_first: bool, actions: &[String]) -> Result<Self, String> {
        match kind {
            Kind::Lighthouse => {
                let acts: Vec<_> = actions
                    .iter()
                    .filter_map(|a| scenario::Action::read(a))
                    .collect();
                Lighthouse::replay(seed, &acts).map(Self::Lighthouse)
            }
            Kind::Station => Station::replay_logged(seed, plan_first, actions).map(Self::Station),
            Kind::Chess => {
                let moves: Vec<String> = actions
                    .iter()
                    .filter_map(|a| a.strip_prefix("move ").map(str::to_owned))
                    .collect();
                Match::replay(&moves).map(Self::Chess)
            }
        }
    }

    /// One action, as written, by a member in `role` (a mission's, or a match's side).
    fn step(&self, role: Option<&str>, written: &str) -> Result<(Self, String), String> {
        match self {
            Self::Lighthouse(now) => {
                let act = scenario::Action::read(written).ok_or(
                    "`action` is look, inspect, test_circuit or replace_part (with part: bulb or \
                     fuse)",
                )?;
                now.step(act)
                    .map(|(next, said)| (Self::Lighthouse(next), said))
            }
            Self::Station(now) => {
                if let Some(text) = written.strip_prefix("plan ") {
                    let role = role
                        .and_then(station::Role::from_name)
                        .ok_or("this task wears no role in the mission")?;
                    return now
                        .post_plan(role, text)
                        .map(|(next, said)| (Self::Station(next), said));
                }
                let act = station::Action::read(written).ok_or(
                    "`action` is look, scan_station, inspect_comms, analyze_anomaly, \
                     route_power, send_probe or attempt_contact",
                )?;
                let role = role
                    .and_then(station::Role::from_name)
                    .ok_or("this task wears no role in the mission")?;
                now.step(role, act)
                    .map(|(next, said)| (Self::Station(next), said))
            }
            Self::Chess(now) => {
                let san = written
                    .strip_prefix("move ")
                    .filter(|s| !s.trim().is_empty())
                    .ok_or("`action` is look, or move with san: the move in standard notation")?;
                let side = role
                    .and_then(Side::from_name)
                    .ok_or("this task plays no side in the match")?;
                now.step(side, san)
                    .map(|(next, said)| (Self::Chess(next), said))
            }
        }
    }

    /// How it ended: its name, and whether that is the work done (a match is, however it
    /// ended: playing it out was the work).
    fn end(&self) -> Option<(&'static str, bool)> {
        match self {
            Self::Lighthouse(now) => now.end().map(|e| (e.name(), e == scenario::End::Solved)),
            Self::Station(now) => now
                .outcome()
                .map(|o| (o.name(), o == station::Outcome::Success)),
            Self::Chess(now) => now.outcome().map(|(result, _)| (result, true)),
        }
    }

    /// Where things stand: every side sees the same.
    fn look(&self) -> String {
        match self {
            Self::Lighthouse(now) => now.look(),
            Self::Station(now) => now.look(),
            Self::Chess(now) => now.look(),
        }
    }

    /// Time spent, in the world's own unit: a match's half-moves.
    fn elapsed(&self) -> u32 {
        match self {
            Self::Lighthouse(now) => now.minutes,
            Self::Station(now) => now.elapsed,
            Self::Chess(now) => u32::try_from(now.moves().len()).unwrap_or(u32::MAX),
        }
    }

    /// Whose move it is, in a match.
    fn to_move(&self) -> Option<&'static str> {
        match self {
            Self::Chess(now) if now.outcome().is_none() => Some(now.to_move().name()),
            _ => None,
        }
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Refuses a project that does not give its work the scenario tool alone.
    pub(super) fn holds_scenarios(&self, project: ProjectId) -> Done<crate::domain::Project> {
        let found = self.project(project)?;
        if self.project_tools(project)?.as_deref() != Some(&[TOOL.to_owned()][..]) {
            return Err(refused(format!(
                "a scenario's project gives its work the {TOOL} tool and nothing else: set \
                 project \"{}\"'s tools to [\"{TOOL}\"] first",
                found.title
            )));
        }
        Ok(found)
    }

    /// The owner starts the lighthouse for `member`, from `seed`, as a task in `project`, which
    /// must give its work the scenario tool and nothing else. Starting it again with the same
    /// seed is its reset: a new task, the same world.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project there is not; [`Refusal::Scenario`] for a project
    /// that does not list the scenario tool alone; what adding the task refuses.
    pub fn start_scenario(&self, project: ProjectId, member: MemberId, seed: u64) -> Done<TaskId> {
        let _held = self.held();
        self.start_scenario_held_lock(project, member, seed)
    }

    /// [`Self::start_scenario`] taken under the engine's lock, as the owner's asking.
    fn start_scenario_held_lock(
        &self,
        project: ProjectId,
        member: MemberId,
        seed: u64,
    ) -> Done<TaskId> {
        let _asked = self.for_owner();
        let found = self.holds_scenarios(project)?;
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

    /// The owner starts Silent Station for a crew of three different members, the captain,
    /// science and the engineer, from `seed`, in `project`: the mission is the captain's task,
    /// and science and the engineer each have a part of it. Starting it again with the same
    /// seed is its reset.
    ///
    /// # Errors
    /// As [`Self::start_scenario`]; [`Refusal::Scenario`] for a crew that is not three
    /// different members.
    pub fn start_station(
        &self,
        project: ProjectId,
        crew: [MemberId; 3],
        seed: u64,
    ) -> Done<TaskId> {
        self.start_station_with(project, crew, seed, false)
    }

    /// The same, and with `plan_first` nothing is done before the captain posts the crew's
    /// plan, as the crew is told: a trial's briefing.
    ///
    /// # Errors
    /// As [`Self::start_station`].
    pub fn start_station_with(
        &self,
        project: ProjectId,
        crew: [MemberId; 3],
        seed: u64,
        plan_first: bool,
    ) -> Done<TaskId> {
        let _held = self.held();
        self.start_station_with_held_lock(project, crew, seed, plan_first)
    }

    /// [`Self::start_station_with`] taken under the engine's lock, as the owner's asking.
    fn start_station_with_held_lock(
        &self,
        project: ProjectId,
        crew: [MemberId; 3],
        seed: u64,
        plan_first: bool,
    ) -> Done<TaskId> {
        let _asked = self.for_owner();
        let found = self.holds_scenarios(project)?;
        let [captain, science, engineer] = crew;
        if captain == science || captain == engineer || science == engineer {
            return Err(refused("a crew is three different members"));
        }
        for member in crew {
            self.member(member)?;
        }
        let task = |title: String, role: station::Role, to: MemberId, parent| NewTask {
            goal: found.goal,
            project: Some(project),
            parent,
            title,
            description: if plan_first {
                station::brief_plan_first(role)
            } else {
                station::brief(role)
            },
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        };
        // The mark first, with no parts, so that the parts' turns, woken as they are added,
        // already find their world.
        let mission = self.add_task_held_lock(
            Author::Owner,
            task(
                format!("Silent Station (seed {seed})"),
                station::Role::Captain,
                captain,
                None,
            ),
        )?;
        let word = if plan_first {
            PLAN_FIRST
        } else {
            station::SILENT_STATION
        };
        self.store
            .set_setting(&key(mission), &format!("{word}:{seed}"))?;
        let mut parts = Vec::new();
        for (role, member) in [
            (station::Role::Science, science),
            (station::Role::Engineer, engineer),
        ] {
            let part = self.add_task_held_lock(
                Author::Owner,
                task(
                    format!("Silent Station: {}", role.name()),
                    role,
                    member,
                    Some(mission),
                ),
            )?;
            self.store
                .set_setting(&key(part), &format!("crew:{}:{}", mission.0, role.name()))?;
            parts.push(part.0.to_string());
        }
        self.store
            .set_setting(&key(mission), &format!("{word}:{seed}:{}", parts.join(":")))?;
        Ok(mission)
    }

    /// The owner starts a chess match between two different members, white and black, in
    /// `project`: the match is white's task, and black has a part of it. `seed` names the game
    /// among a trial's (chess has no chance in it). Black's first turn finds it is white's move
    /// and ends; each legal move wakes the other side.
    ///
    /// # Errors
    /// As [`Self::start_scenario`]; [`Refusal::Scenario`] for one member on both sides.
    pub fn start_chess(
        &self,
        project: ProjectId,
        players: [MemberId; 2],
        seed: u64,
    ) -> Done<TaskId> {
        let _held = self.held();
        self.start_chess_held_lock(project, players, seed)
    }

    /// [`Self::start_chess`] taken under the engine's lock, as the owner's asking.
    pub(super) fn start_chess_held_lock(
        &self,
        project: ProjectId,
        players: [MemberId; 2],
        seed: u64,
    ) -> Done<TaskId> {
        let _asked = self.for_owner();
        let found = self.holds_scenarios(project)?;
        let [white, black] = players;
        if white == black {
            return Err(refused("a match is two different members"));
        }
        for member in players {
            self.member(member)?;
        }
        let task = |title: String, side: Side, to: MemberId, parent| NewTask {
            goal: found.goal,
            project: Some(project),
            parent,
            title,
            description: chess_match::brief(side),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        };
        let game = self.add_task_held_lock(
            Author::Owner,
            task(format!("Chess (game {seed})"), Side::White, white, None),
        )?;
        self.store
            .set_setting(&key(game), &format!("{}:{seed}", chess_match::CHESS))?;
        let part = self.add_task_held_lock(
            Author::Owner,
            task(
                format!("Chess (game {seed}): black"),
                Side::Black,
                black,
                Some(game),
            ),
        )?;
        self.store.set_setting(
            &key(part),
            &format!("crew:{}:{}", game.0, Side::Black.name()),
        )?;
        self.store.set_setting(
            &key(game),
            &format!("{}:{seed}:{}", chess_match::CHESS, part.0),
        )?;
        Ok(game)
    }

    /// Where a task stands in a scenario, if it is in one.
    fn place_of(&self, task: TaskId) -> Done<Option<Place>> {
        let Some(kept) = self.store.setting(&key(task))? else {
            return Ok(None);
        };
        let words: Vec<&str> = kept.split(':').collect();
        let place = match words.as_slice() {
            [seed] => seed.parse().ok().map(|seed| Place {
                world: task,
                kind: Kind::Lighthouse,
                seed,
                parts: Vec::new(),
                role: None,
                plan_first: false,
            }),
            [kind, seed, parts @ ..]
                if *kind == station::SILENT_STATION
                    || *kind == PLAN_FIRST
                    || *kind == chess_match::CHESS =>
            {
                let plan_first = *kind == PLAN_FIRST;
                let kind = if *kind == chess_match::CHESS {
                    Kind::Chess
                } else {
                    Kind::Station
                };
                seed.parse().ok().map(|seed| Place {
                    world: task,
                    kind,
                    seed,
                    parts: parts
                        .iter()
                        .filter_map(|p| p.parse().ok().map(TaskId))
                        .collect(),
                    role: kind.holder(),
                    plan_first,
                })
            }
            ["crew", mission, role] => {
                let Some(mission) = mission.parse().ok().map(TaskId) else {
                    return Ok(None);
                };
                let Some(at) = self.place_of_world(mission)? else {
                    return Ok(None);
                };
                Some(Place {
                    role: at.kind.role(role),
                    ..at
                })
            }
            _ => None,
        };
        Ok(place)
    }

    /// A shared world's own place, never a part's: what a part points at.
    fn place_of_world(&self, mission: TaskId) -> Done<Option<Place>> {
        Ok(self
            .place_of(mission)?
            .filter(|p| p.world == mission && p.kind != Kind::Lighthouse))
    }

    /// A task's scenario, by its seed, if it is in one.
    ///
    /// # Errors
    /// A store failure.
    pub fn scenario_of(&self, task: TaskId) -> Done<Option<u64>> {
        Ok(self.place_of(task)?.map(|p| p.seed))
    }

    /// Which world a turn's work is in, by name, if any: the scenario tool it is offered.
    pub(crate) fn scenario_kind_of_run(&self, run: RunId) -> Done<Option<&'static str>> {
        let Some(found) = self.store.run(run)? else {
            return Ok(None);
        };
        let Some(task) = found.task else {
            return Ok(None);
        };
        // Offered only to the member who plays that task: anyone else woken on it is not.
        if self.player_of(task)? != Some(found.member) {
            return Ok(None);
        }
        Ok(self.place_of(task)?.map(|p| p.kind.name()))
    }

    /// The member who plays a scenario's task: its assignee, or its owner.
    fn player_of(&self, task: TaskId) -> Done<Option<MemberId>> {
        Ok(self.store.task(task)?.and_then(|t| t.assignee.or(t.owner)))
    }

    /// Whether a task is in a scenario: its world wakes its players, so the stuck-work guards
    /// and the ladder leave it alone.
    pub(super) fn in_a_scenario(&self, task: TaskId) -> Done<bool> {
        Ok(self.store.setting(&key(task))?.is_some())
    }

    /// The owner runs the Holodeck again (2026-10-25: so the team re-attempts the whole
    /// testing set). Every finished scenario not yet in a round is labelled `round`, and kept;
    /// each distinct setup among them (its world, seed, players and plan) is queued to start
    /// again, oldest first, one at a time (see [`Self::holodeck_rerun_tick`]). Returns how many
    /// are queued.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for an empty label; [`Refusal::Scenario`] while a re-run is
    /// queued or a scenario is still running; a store failure.
    pub fn rerun_holodeck(&self, round: &str) -> Done<usize> {
        let _held = self.held();
        let _asked = self.for_owner();
        let round = super::one_line(round, 60);
        if round.is_empty() {
            return Err(EngineError::Unknown("round label"));
        }
        if !self.rerun_queue()?.is_empty() {
            return Err(refused(
                "the Holodeck is already running its scenarios again",
            ));
        }
        let mut tasks = self.store.tasks()?;
        tasks.sort_by_key(|t| t.id);
        let mut archived = Vec::new();
        let mut setups: Vec<String> = Vec::new();
        for task in tasks.iter().filter(|t| t.parent.is_none()) {
            // Chess is the league's, reset on its own: an old match is never queued again.
            let Some(place) = self
                .place_of(task.id)?
                .filter(|p| p.world == task.id && p.kind != Kind::Chess)
            else {
                continue;
            };
            if !task.status.finished() {
                return Err(refused("a scenario is still running: let it end first"));
            }
            if self.store.setting(&round_key(task.id))?.is_some() {
                continue;
            }
            let setup = self.setup_of(task, &place)?;
            if !setups.contains(&setup) {
                setups.push(setup);
            }
            archived.push(task.id);
        }
        for task in archived {
            self.store.set_setting(&round_key(task), &round)?;
        }
        self.store.set_setting(RERUN_QUEUE, &setups.join(";"))?;
        Ok(setups.len())
    }

    /// The owner starts the re-run over, on the engine as it is now (2026-10-25): the scenario
    /// being played is dropped (kept, never deleted), every world not yet in a round is
    /// labelled `round`, and every distinct setup there has ever been, earlier rounds'
    /// included, is queued again, oldest first, one at a time. Returns how many are queued.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for an empty label; a store failure.
    pub fn restart_holodeck(&self, round: &str) -> Done<usize> {
        let _held = self.held();
        let _asked = self.for_owner();
        let round = super::one_line(round, 60);
        if round.is_empty() {
            return Err(EngineError::Unknown("round label"));
        }
        let mut tasks = self.store.tasks()?;
        tasks.sort_by_key(|t| t.id);
        let mut setups: Vec<String> = Vec::new();
        for task in tasks.iter().filter(|t| t.parent.is_none()) {
            // Chess is the league's, reset on its own: an old match is never queued again.
            let Some(place) = self
                .place_of(task.id)?
                .filter(|p| p.world == task.id && p.kind != Kind::Chess)
            else {
                continue;
            };
            if !task.status.finished() {
                for world in std::iter::once(task.id).chain(place.parts.iter().copied()) {
                    if !self.task(world)?.status.finished() {
                        self.store
                            .set_task_status(world, crate::domain::TaskStatus::Dropped)?;
                    }
                }
            }
            if self.store.setting(&round_key(task.id))?.is_none() {
                self.store.set_setting(&round_key(task.id), &round)?;
            }
            let setup = self.setup_of(task, &place)?;
            if !setups.contains(&setup) {
                setups.push(setup);
            }
        }
        self.store.set_setting(RERUN_QUEUE, &setups.join(";"))?;
        Ok(setups.len())
    }

    /// A scenario's setup, as the re-run queue keeps it: `kind:seed:project:plan:players`.
    fn setup_of(&self, task: &crate::domain::Task, place: &Place) -> Done<String> {
        let holder = |id: TaskId| -> Done<i64> {
            Ok(self
                .task(id)?
                .assignee
                .ok_or(EngineError::Unknown("a scenario's player"))?
                .0)
        };
        let mut players = vec![holder(task.id)?];
        for part in &place.parts {
            players.push(holder(*part)?);
        }
        Ok(format!(
            "{}:{}:{}:{}:{}",
            place.kind.name(),
            place.seed,
            task.project.map_or(0, |p| p.0),
            u8::from(place.plan_first),
            players
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ))
    }

    /// The scenarios waiting to run again, in order.
    pub(super) fn rerun_queue(&self) -> Done<Vec<String>> {
        Ok(self
            .store
            .setting(RERUN_QUEUE)?
            .map(|kept| {
                kept.split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// How many scenarios wait to run again, for the screen.
    pub(super) fn rerun_queued(&self) -> Done<usize> {
        Ok(self.rerun_queue()?.len())
    }

    /// Whether a scenario's world is being played: one at a time, for the re-run and the league.
    pub(super) fn a_world_is_live(&self) -> Done<bool> {
        for task in self.store.tasks()? {
            if task.parent.is_none()
                && !task.status.finished()
                && self.place_of(task.id)?.is_some_and(|p| p.world == task.id)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// On the tick: when no scenario is running, the next one queued to run again starts. A
    /// setup that no longer starts (a player let go, a project changed) is passed over.
    pub(super) fn holodeck_rerun_tick(&self) -> Done<()> {
        let mut queue = self.rerun_queue()?;
        if queue.is_empty() || self.a_world_is_live()? {
            return Ok(());
        }
        let next = queue.remove(0);
        self.store.set_setting(RERUN_QUEUE, &queue.join(";"))?;
        let words: Vec<&str> = next.split(':').collect();
        let [kind, seed, project, plan, players] = words.as_slice() else {
            return Ok(());
        };
        let (Ok(seed), Ok(project)) = (seed.parse::<u64>(), project.parse::<i64>()) else {
            return Ok(());
        };
        let project = ProjectId(project);
        let players: Vec<MemberId> = players
            .split(',')
            .filter_map(|p| p.parse().ok().map(MemberId))
            .collect();
        let started = match (*kind, players.as_slice()) {
            (scenario::LIGHTHOUSE, [member]) => {
                self.start_scenario_held_lock(project, *member, seed)
            }
            (station::SILENT_STATION, [a, b, c]) => {
                self.start_station_with_held_lock(project, [*a, *b, *c], seed, *plan == "1")
            }
            (chess_match::CHESS, [white, black]) => {
                self.start_chess_held_lock(project, [*white, *black], seed)
            }
            _ => return Ok(()),
        };
        // A setup that no longer starts is passed over; the next tick takes the one after.
        let _ = started;
        Ok(())
    }

    /// What the tick does for the scenarios: a world that has ended with its tasks still open
    /// (an end a rule added after its last action gives it) is closed; and a world whose
    /// players have no turn coming, and that has sat idle, has them woken again, three times at
    /// most, and is then abandoned. Live, a turn stopped for its time had folded into it the
    /// wake that would have brought its player back, and a match waited for ever.
    pub(super) fn close_ended_scenarios(&self) -> Done<()> {
        let runs = self.store.runs()?;
        for listed in self.scenarios_listed(usize::MAX)? {
            if listed["status"] == "done" || listed["status"] == "dropped" {
                continue;
            }
            let task = TaskId(listed["task"].as_i64().unwrap_or_default());
            let Some(place) = self.place_of(task)? else {
                continue;
            };
            let now = self.world_at(&place)?;
            if let Some((name, done)) = now.end() {
                self.scenario_ended(&place, name, done, &now.look())?;
                continue;
            }
            self.wake_the_idle(&place, &now, &runs)?;
        }
        Ok(())
    }

    /// Wakes a world's players again when none has a turn coming and it has sat idle since its
    /// last action or its last such wake; after [`REWAKES`] wakes with nothing done, abandons
    /// it. Kept as a setting a world: `<actions logged>:<wakes>:<when>`.
    fn wake_the_idle(
        &self,
        place: &Place,
        now: &World,
        runs: &[crate::application::ports::Run],
    ) -> Done<()> {
        let tasks: Vec<TaskId> = std::iter::once(place.world)
            .chain(place.parts.iter().copied())
            .collect();
        let coming = runs.iter().any(|r| {
            r.task.is_some_and(|t| tasks.contains(&t))
                && matches!(
                    r.status,
                    crate::domain::RunStatus::Queued
                        | crate::domain::RunStatus::Running
                        | crate::domain::RunStatus::RetryWaiting
                )
        });
        let mut held = false;
        for task in &tasks {
            held |= !self.store.held_wakes(*task)?.is_empty();
        }
        if coming || held {
            return Ok(());
        }
        let log = self.store.scenario_actions(place.world)?;
        let last_act = log.last().map_or(0, |a| a.at);
        let logged_now = i64::try_from(log.len()).unwrap_or(i64::MAX);
        // The idle clock starts when the tick first sees the world, not at the epoch.
        let Some(kept) = self.store.setting(&idle_key(place.world))? else {
            self.store.set_setting(
                &idle_key(place.world),
                &format!("{logged_now}:0:{}", self.clock.now()),
            )?;
            return Ok(());
        };
        let mut parts = kept.split(':').map(|p| p.parse::<i64>().unwrap_or(0));
        let (seen, wakes, at) = (
            parts.next().unwrap_or(-1),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        );
        let logged = i64::try_from(log.len()).unwrap_or(i64::MAX);
        let (wakes, since) = if seen == logged {
            (wakes, at.max(last_act))
        } else {
            (0, last_act)
        };
        let now_ms = self.clock.now();
        if since > 0 && now_ms - since < IDLE_MS {
            if seen != logged {
                self.store
                    .set_setting(&idle_key(place.world), &format!("{logged}:0:{since}"))?;
            }
            return Ok(());
        }
        if wakes >= REWAKES {
            self.scenario_ended(
                place,
                "abandoned",
                false,
                &format!("nobody acted in it after {REWAKES} wakes"),
            )?;
            return Ok(());
        }
        // Who is to act: a match's side to move; otherwise every player still at it.
        let to_wake: Vec<TaskId> = match now.to_move() {
            Some(side) => tasks
                .iter()
                .copied()
                .filter(|t| {
                    self.place_of(*t)
                        .ok()
                        .flatten()
                        .is_some_and(|p| p.role == Some(side))
                })
                .collect(),
            None => tasks.clone(),
        };
        for task in to_wake {
            let Some(found) = self.store.task(task)? else {
                continue;
            };
            if found.status.finished() {
                continue;
            }
            if let Some(player) = found.assignee.or(found.owner) {
                self.wake_held_lock(player, Some(task), WakeReason::Mentioned)?;
            }
        }
        self.store.set_setting(
            &idle_key(place.world),
            &format!("{logged}:{}:{now_ms}", wakes + 1),
        )?;
        Ok(())
    }

    /// The accepted actions of a world's log, in order.
    fn accepted(&self, world: TaskId) -> Done<Vec<String>> {
        Ok(self
            .store
            .scenario_actions(world)?
            .into_iter()
            .filter(|a| a.ok)
            .map(|a| a.action)
            .collect())
    }

    fn world_at(&self, place: &Place) -> Done<World> {
        World::replay(
            place.kind,
            place.seed,
            place.plan_first,
            &self.accepted(place.world)?,
        )
        .map_err(refused)
    }

    /// A lighthouse as its log replays.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no lighthouse; [`Refusal::Scenario`] for a
    /// log that does not replay, which an append-only log written by this engine never is.
    pub fn replay_scenario(&self, task: TaskId) -> Done<Lighthouse> {
        let place = self
            .place_of(task)?
            .filter(|p| p.kind == Kind::Lighthouse)
            .ok_or(EngineError::Unknown("scenario"))?;
        match self.world_at(&place)? {
            World::Lighthouse(now) => Ok(now),
            World::Station(_) | World::Chess(_) => Err(EngineError::Unknown("scenario")),
        }
    }

    /// A mission of Silent Station as its log replays.
    ///
    /// # Errors
    /// As [`Self::replay_scenario`], for a task in no mission.
    pub fn replay_station(&self, task: TaskId) -> Done<Station> {
        let place = self
            .place_of(task)?
            .filter(|p| p.kind == Kind::Station)
            .ok_or(EngineError::Unknown("scenario"))?;
        match self.world_at(&place)? {
            World::Station(now) => Ok(now),
            World::Lighthouse(_) | World::Chess(_) => Err(EngineError::Unknown("scenario")),
        }
    }

    /// A chess match as its log replays.
    ///
    /// # Errors
    /// As [`Self::replay_scenario`], for a task in no match.
    pub fn replay_chess(&self, task: TaskId) -> Done<Match> {
        let place = self
            .place_of(task)?
            .filter(|p| p.kind == Kind::Chess)
            .ok_or(EngineError::Unknown("scenario"))?;
        match self.world_at(&place)? {
            World::Chess(now) => Ok(now),
            World::Lighthouse(_) | World::Station(_) => Err(EngineError::Unknown("scenario")),
        }
    }

    /// Who wears which role in a world: the holder of its own task and its parts' members.
    fn roles_in(&self, place: &Place) -> Done<HashMap<MemberId, &'static str>> {
        let mut roles = HashMap::new();
        if place.kind == Kind::Lighthouse {
            return Ok(roles);
        }
        for task in std::iter::once(place.world).chain(place.parts.iter().copied()) {
            let Some(found) = self.store.task(task)? else {
                continue;
            };
            let Some(role) = self.place_of(task)?.and_then(|p| p.role) else {
                continue;
            };
            if let Some(member) = found.assignee.or(found.owner) {
                roles.insert(member, role);
            }
        }
        Ok(roles)
    }

    /// A scenario as the owner's screen shows it: its seed, where it stands, how it ended and
    /// what it scored, and, once it has ended, what was hidden; and its log step by step,
    /// refused actions too, each with who acted, as what, and what the world said.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no scenario; a store failure.
    pub fn scenario_shown(&self, task: TaskId) -> Done<Value> {
        let place = self
            .place_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let now = self.world_at(&place)?;
        let names = self.names()?;
        let roles = self.roles_in(&place)?;
        let log: Vec<Value> = self
            .store
            .scenario_actions(place.world)?
            .iter()
            .map(|a| {
                json!({ "action": a.action, "ok": a.ok, "said": a.said, "run": a.run.0,
                        "member": a.member.0, "by": names.get(&a.member),
                        "role": roles.get(&a.member), "at": a.at })
            })
            .collect();
        let mut shown = json!({ "task": place.world.0, "kind": place.kind.name(),
                                "seed": place.seed, "end": now.end().map(|(name, _)| name),
                                "elapsed": now.elapsed(), "said": now.look(),
                                "status": self.task(place.world)?.status.name(),
                                "log": log });
        let over = now.end().is_some();
        match &now {
            World::Lighthouse(lamp) => {
                shown["minutes"] = json!(lamp.minutes);
                shown["limit"] = json!(scenario::TIME_LIMIT);
                shown["lit"] = json!(lamp.lit);
                shown["cause"] = json!(over.then_some(match lamp.cause {
                    scenario::Cause::Bulb => "bulb",
                    scenario::Cause::Fuse => "fuse",
                }));
            }
            World::Station(at) => {
                shown["score"] = json!(at.score());
                shown["anomaly"] = json!(over.then_some(at.anomaly.name()));
                shown["plan"] = json!(at.plan);
                shown["brief"] = json!(if at.plan_first {
                    "plan_first"
                } else {
                    "standard"
                });
                shown["state"] = json!({ "station_power": at.station_power,
                    "life_support": at.life_support, "ship_energy": at.ship_energy,
                    "hazard": at.hazard, "crew_safe": at.crew_safe,
                    "power_routed": at.power_routed, "probe_out": at.probe_out,
                    "contact": at.contact });
                shown["crew"] = json!(
                    roles
                        .iter()
                        .map(|(m, r)| ((*r).to_owned(), json!(names.get(m))))
                        .collect::<serde_json::Map<_, _>>()
                );
            }
            World::Chess(game) => {
                shown["score"] = json!(game.score());
                shown["result"] = json!(game.outcome().map(|(r, _)| r));
                shown["reason"] = json!(game.outcome().map(|(_, why)| why));
                shown["moves"] = json!(game.move_text());
                shown["to_move"] = json!(now.to_move());
                if let Some((fen, rows)) = game.board() {
                    shown["fen"] = json!(fen);
                    shown["rows"] = json!(rows);
                }
                shown["sides"] = json!(
                    roles
                        .iter()
                        .map(|(m, r)| ((*r).to_owned(), json!(names.get(m))))
                        .collect::<serde_json::Map<_, _>>()
                );
            }
        }
        shown["by_role"] = json!(self.by_role(&place, &roles)?);
        Ok(shown)
    }

    /// Each role's actions in a world, accepted and refused, and who wore it: a match's legal
    /// and illegal moves by side, for the owner and for comparing brains.
    fn by_role(&self, place: &Place, roles: &HashMap<MemberId, &'static str>) -> Done<Vec<Value>> {
        let log = self.store.scenario_actions(place.world)?;
        let names = self.names()?;
        let mut worn: Vec<(&'static str, MemberId)> = roles.iter().map(|(m, r)| (*r, *m)).collect();
        worn.sort_unstable();
        Ok(worn
            .into_iter()
            .map(|(role, member)| {
                let mine = log.iter().filter(|a| a.member == member);
                let accepted = mine.clone().filter(|a| a.ok).count();
                let refused = mine.filter(|a| !a.ok).count();
                json!({ "role": role, "member": member.0, "by": names.get(&member),
                        "accepted": accepted, "refused": refused })
            })
            .collect())
    }

    /// The scenarios as the owner's screen lists them, newest first, the latest `most`: each
    /// world's task, kind, title and status, how it ended and its time. Read from the projects
    /// that give their work the scenario tool alone, where every scenario is.
    ///
    /// # Errors
    /// A store failure.
    pub(super) fn scenarios_listed(&self, most: usize) -> Done<Vec<Value>> {
        let mut holding = Vec::new();
        for project in self.store.projects()? {
            if self.project_tools(project.id)?.as_deref() == Some(&[TOOL.to_owned()][..]) {
                holding.push(project.id);
            }
        }
        let mut tasks: Vec<_> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|t| t.parent.is_none() && t.project.is_some_and(|p| holding.contains(&p)))
            .collect();
        tasks.sort_by_key(|t| std::cmp::Reverse(t.id));
        let mut listed = Vec::new();
        for task in tasks {
            if listed.len() >= most {
                break;
            }
            let Some(place) = self.place_of(task.id)?.filter(|p| p.world == task.id) else {
                continue;
            };
            let now = self.world_at(&place)?;
            listed.push(
                json!({ "task": task.id.0, "kind": place.kind.name(), "title": task.title,
                                "status": task.status.name(),
                                "end": now.end().map(|(name, _)| name),
                                "elapsed": now.elapsed(),
                                "round": self.store.setting(&round_key(task.id))? }),
            );
        }
        Ok(listed)
    }

    /// What the record says of one scenario, for a trial: its world, how it ended, its score
    /// and time, its accepted and refused actions, whether anyone was hurt, and the tasks its
    /// turns were taken on.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no scenario; a store failure.
    pub(super) fn scenario_facts(&self, task: TaskId) -> Done<Facts> {
        let place = self
            .place_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let now = self.world_at(&place)?;
        let log = self.store.scenario_actions(place.world)?;
        let count =
            |ok: bool| u32::try_from(log.iter().filter(|a| a.ok == ok).count()).unwrap_or(0);
        let (score, unsafe_act) = match &now {
            World::Lighthouse(lamp) => (if lamp.lit { 100 } else { 0 }, false),
            World::Station(at) => (at.score(), !at.crew_safe),
            World::Chess(game) => (game.score(), false),
        };
        let roles = self.roles_in(&place)?;
        Ok(Facts {
            end: now.end().map(|(name, _)| name),
            done: now.end().is_some_and(|(_, done)| done),
            score,
            elapsed: now.elapsed(),
            accepted: count(true),
            refused: count(false),
            unsafe_act,
            tasks: std::iter::once(place.world)
                .chain(place.parts.iter().copied())
                .collect(),
            by_role: self.by_role(&place, &roles)?,
        })
    }

    /// Why a turn is refused one of HQ's tools on a scenario, if it is: moving a scenario's
    /// task, or a part of a mission, is the world's, never a member's word.
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
        if self.place_of(TaskId(task))?.is_none() {
            return Ok(None);
        }
        Ok(Some(
            "a scenario is closed by the world, not by anyone's word: act in it with the \
             scenario tool, and it closes itself when it ends"
                .to_owned(),
        ))
    }

    /// `scenario`: `look` says where things stand; any other action is the world's to accept
    /// or refuse, as the turn's role in a mission. Every action is kept in the world's log,
    /// refused ones too, and said as an event; an accepted one wakes the rest of a crew, and
    /// one that ends the world closes its tasks.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::Scenario`] for a turn whose task is no scenario, an
    /// action the world does not know, or one it refuses; a store failure.
    pub fn scenario_in(&self, run: RunId, action: &str, part: Option<&str>) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let no_scenario = || refused("this turn's task is no scenario: there is nothing to act in");
        let task = turn.task.ok_or_else(no_scenario)?;
        let place = self.place_of(task)?.ok_or_else(no_scenario)?;
        // A role is its task's member's: anyone else woken on the task does not play it.
        if let Some(player) = self.player_of(task)?.filter(|p| *p != turn.member) {
            return Err(refused(format!(
                "this task is {}'s to play in the world, not yours",
                self.member(player)?.name
            )));
        }
        let now = self.world_at(&place)?;
        if action.trim() == "look" {
            let seen = now.look();
            // A side that looks on the other's move has nothing to do until it is woken.
            if now.to_move().is_some_and(|side| Some(side) != place.role) {
                return Ok(format!(
                    "{seen}\nIt is not your move: this turn has ended, and you are woken when \
                     it is."
                ));
            }
            return Ok(seen);
        }
        let written = match part {
            Some(part) if action.trim() == "replace_part" => {
                format!("replace_part {}", part.trim())
            }
            Some(san) if action.trim() == "move" => format!("move {}", san.trim()),
            Some(text) if action.trim() == "plan" => format!("plan {}", text.trim()),
            _ => action.trim().to_owned(),
        };
        let kept: String = written.chars().take(ACTION_MOST).collect();
        // What is kept is what is played, so the log replays to the same world.
        let (next, said) = match now.step(place.role, &kept) {
            Ok(done) => done,
            Err(why) => {
                self.act_kept(&place, run, turn.member, &kept, false, &why, &now)?;
                return Err(refused(why));
            }
        };
        self.act_kept(&place, run, turn.member, &kept, true, &said, &next)?;
        if let Some(mut found) = self.store.run(run)? {
            found.moved_forward = true;
            self.store.save_run(&found)?;
        }
        if let Some((name, done)) = next.end() {
            self.scenario_ended(&place, name, done, &said)?;
            // The worker ends a turn HQ says has ended: live, models went on to set_status.
            return Ok(format!(
                "{said} The world has closed your task, and this turn has ended: stop now."
            ));
        }
        self.crew_told(&place, turn.member)?;
        // A match is a move a turn: the mover's turn ends, and the other side is woken.
        if place.kind == Kind::Chess {
            return Ok(format!(
                "{said} Your move is made: this turn has ended, and you are woken when it is \
                 your move again."
            ));
        }
        Ok(said)
    }

    /// The rest of a crew, or a match's other side, is woken on its own part, to act on what
    /// changed.
    fn crew_told(&self, place: &Place, actor: MemberId) -> Done<()> {
        if place.kind == Kind::Lighthouse {
            return Ok(());
        }
        for task in std::iter::once(place.world).chain(place.parts.iter().copied()) {
            let Some(found) = self.store.task(task)? else {
                continue;
            };
            let Some(member) = found.assignee.or(found.owner) else {
                continue;
            };
            if member == actor || found.status.finished() {
                continue;
            }
            self.wake_held_lock(member, Some(task), WakeReason::Mentioned)?;
        }
        Ok(())
    }

    /// One action into the world's log, and its event.
    #[allow(clippy::too_many_arguments)]
    fn act_kept(
        &self,
        place: &Place,
        run: RunId,
        member: MemberId,
        action: &str,
        ok: bool,
        said: &str,
        after: &World,
    ) -> Done<()> {
        self.store.add_scenario_action(&ScenarioAction {
            id: 0,
            task: place.world,
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
            json!({ "task": place.world.0, "kind": place.kind.name(), "run": run.0,
                    "member": member.0, "by": by, "role": place.role,
                    "action": action, "ok": ok, "said": said, "elapsed": after.elapsed(),
                    "end": after.end().map(|(name, _)| name) }),
        )
    }

    /// The world closes its tasks: done when it ended well, dropped (it cannot go on)
    /// otherwise. A mission's parts close with it, after it, so nothing wakes its captain.
    fn scenario_ended(&self, place: &Place, name: &str, done: bool, said: &str) -> Done<()> {
        let (status, closure) = if done {
            (TaskStatus::Done, Closure::Done(DoneWhy::MetCriteria))
        } else {
            (
                TaskStatus::Dropped,
                Closure::Dropped(DropWhy::BlockedForever),
            )
        };
        let world = self.task(place.world)?;
        self.store.set_task_status(world.id, status)?;
        self.emit_status(&world, status)?;
        self.closed(world.id, closure, None, "endora")?;
        self.post_held_lock(
            Author::Endora,
            world.id,
            &format!("The scenario is over, {name}: {said}"),
        )?;
        for part in &place.parts {
            let found = self.task(*part)?;
            if found.status.finished() {
                continue;
            }
            self.store.set_task_status(found.id, status)?;
            self.emit_status(&found, status)?;
            self.closed(found.id, closure, None, "endora")?;
        }
        self.after_finishing(&world)
    }
}
