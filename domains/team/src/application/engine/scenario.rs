//! Scenarios (the Holodeck plan's phases 5 and 6, ADR 0135): the owner starts one as a task in
//! a project that gives its work the scenario tool alone; members act through that tool; the
//! world, in code, says what each action did and closes its tasks when it ends. The state is
//! never kept: it is the seed's world with the log's accepted actions replayed, and the log is
//! append-only, so a reset is a new task with the same seed.
//!
//! Two worlds. The lighthouse is one member's task. Silent Station is a crew's: the captain
//! holds the mission, science and the engineer a part of it each, all acting in the mission's
//! one world and log, each as its role alone; an accepted action wakes the rest of the crew.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, NewTask, ScenarioAction, TeamStore, WorkerAdapter};
use crate::domain::closure::{Closure, DoneWhy, DropWhy};
use crate::domain::scenario::{self, Lighthouse};
use crate::domain::station::{self, Station};
use crate::domain::{
    EventKind, MemberId, Priority, ProjectId, Refusal, RunId, TaskId, TaskStatus, WakeReason,
};

/// The tool's name, which a scenario's project lists alone.
pub const TOOL: &str = "scenario";

/// The longest action, as written, the log keeps.
const ACTION_MOST: usize = 60;

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

/// Where a task's place in a scenario is kept: `<seed>` for a lighthouse;
/// `silent_station:<seed>:<science's task>:<engineer's task>` for a mission;
/// `crew:<mission>:<role>` for a part of one.
fn key(task: TaskId) -> String {
    format!("scenario:{}", task.0)
}

/// Which world a scenario is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The lighthouse (phase 5).
    Lighthouse,
    /// Silent Station (phase 6).
    Station,
}

impl Kind {
    /// Its name.
    const fn name(self) -> &'static str {
        match self {
            Self::Lighthouse => scenario::LIGHTHOUSE,
            Self::Station => station::SILENT_STATION,
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
    /// The mission's parts, science's then the engineer's.
    parts: Vec<TaskId>,
    /// The role this task's member wears, in a mission.
    role: Option<station::Role>,
}

/// A world as it stands, whichever it is.
enum World {
    Lighthouse(Lighthouse),
    Station(Station),
}

impl World {
    fn replay(kind: Kind, seed: u64, actions: &[String]) -> Result<Self, String> {
        match kind {
            Kind::Lighthouse => {
                let acts: Vec<_> = actions
                    .iter()
                    .filter_map(|a| scenario::Action::read(a))
                    .collect();
                Lighthouse::replay(seed, &acts).map(Self::Lighthouse)
            }
            Kind::Station => {
                let acts: Vec<_> = actions
                    .iter()
                    .filter_map(|a| station::Action::read(a))
                    .collect();
                Station::replay(seed, &acts).map(Self::Station)
            }
        }
    }

    /// One action, as written, by a member in `role` (a mission's).
    fn step(&self, role: Option<station::Role>, written: &str) -> Result<(Self, String), String> {
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
                let act = station::Action::read(written).ok_or(
                    "`action` is look, scan_station, inspect_comms, analyze_anomaly, \
                     route_power, send_probe or attempt_contact",
                )?;
                let role = role.ok_or("this task wears no role in the mission")?;
                now.step(role, act)
                    .map(|(next, said)| (Self::Station(next), said))
            }
        }
    }

    /// How it ended: its name, and whether that is the work done.
    fn end(&self) -> Option<(&'static str, bool)> {
        match self {
            Self::Lighthouse(now) => now.end().map(|e| (e.name(), e == scenario::End::Solved)),
            Self::Station(now) => now
                .outcome()
                .map(|o| (o.name(), o == station::Outcome::Success)),
        }
    }

    fn look(&self) -> String {
        match self {
            Self::Lighthouse(now) => now.look(),
            Self::Station(now) => now.look(),
        }
    }

    /// Time spent, in the world's own unit.
    fn elapsed(&self) -> u32 {
        match self {
            Self::Lighthouse(now) => now.minutes,
            Self::Station(now) => now.elapsed,
        }
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Refuses a project that does not give its work the scenario tool alone.
    fn holds_scenarios(&self, project: ProjectId) -> Done<crate::domain::Project> {
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
        let _held = self.held();
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
            description: station::brief(role),
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
        self.store.set_setting(
            &key(mission),
            &format!("{}:{seed}", station::SILENT_STATION),
        )?;
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
        self.store.set_setting(
            &key(mission),
            &format!("{}:{seed}:{}", station::SILENT_STATION, parts.join(":")),
        )?;
        Ok(mission)
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
            }),
            [kind, seed, parts @ ..] if *kind == station::SILENT_STATION => {
                seed.parse().ok().map(|seed| Place {
                    world: task,
                    kind: Kind::Station,
                    seed,
                    parts: parts
                        .iter()
                        .filter_map(|p| p.parse().ok().map(TaskId))
                        .collect(),
                    role: Some(station::Role::Captain),
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
                    role: station::Role::from_name(role),
                    ..at
                })
            }
            _ => None,
        };
        Ok(place)
    }

    /// A mission's own place, never a part's: what a part points at.
    fn place_of_world(&self, mission: TaskId) -> Done<Option<Place>> {
        Ok(self
            .place_of(mission)?
            .filter(|p| p.world == mission && p.kind == Kind::Station))
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
        let Some(task) = self.store.run(run)?.and_then(|r| r.task) else {
            return Ok(None);
        };
        Ok(self.place_of(task)?.map(|p| p.kind.name()))
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
        World::replay(place.kind, place.seed, &self.accepted(place.world)?).map_err(refused)
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
            World::Station(_) => Err(EngineError::Unknown("scenario")),
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
            World::Lighthouse(_) => Err(EngineError::Unknown("scenario")),
        }
    }

    /// Who wears which role in a world: the mission's captain and its parts' members.
    fn roles_in(&self, place: &Place) -> Done<HashMap<MemberId, station::Role>> {
        let mut roles = HashMap::new();
        if place.kind != Kind::Station {
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
                        "role": roles.get(&a.member).map(|r| r.name()), "at": a.at })
            })
            .collect();
        let mut shown = json!({ "task": place.world.0, "kind": place.kind.name(),
                                "seed": place.seed, "end": now.end().map(|(name, _)| name),
                                "elapsed": now.elapsed(), "said": now.look(), "log": log });
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
                shown["state"] = json!({ "station_power": at.station_power,
                    "life_support": at.life_support, "ship_energy": at.ship_energy,
                    "hazard": at.hazard, "crew_safe": at.crew_safe,
                    "power_routed": at.power_routed, "probe_out": at.probe_out,
                    "contact": at.contact });
                shown["crew"] = json!(
                    roles
                        .iter()
                        .map(|(m, r)| (r.name().to_owned(), json!(names.get(m))))
                        .collect::<serde_json::Map<_, _>>()
                );
            }
        }
        Ok(shown)
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
        let now = self.world_at(&place)?;
        if action.trim() == "look" {
            return Ok(now.look());
        }
        let written = match part {
            Some(part) if action.trim() == "replace_part" => {
                format!("replace_part {}", part.trim())
            }
            _ => action.trim().to_owned(),
        };
        let kept: String = written.chars().take(ACTION_MOST).collect();
        let (next, said) = match now.step(place.role, &written) {
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
            return Ok(said);
        }
        self.crew_told(&place, turn.member)?;
        Ok(said)
    }

    /// The rest of a crew is woken on its own part, to act on what changed.
    fn crew_told(&self, place: &Place, actor: MemberId) -> Done<()> {
        if place.kind != Kind::Station {
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
                    "member": member.0, "by": by, "role": place.role.map(station::Role::name),
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
