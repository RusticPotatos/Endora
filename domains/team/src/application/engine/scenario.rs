//! Scenarios (ADR 0135): the owner starts one as a task in a project that gives its work the
//! scenario tool alone; members act through that tool; the world, in code, says what each
//! action did and closes its tasks when it ends. The state is never kept: it is the world's
//! start with the log's accepted actions replayed, and the log is append-only.
//!
//! One world is left: a chess match, two members', white holding the match and black a part of
//! it, a move a turn, the opponent woken when it is its move. The lighthouse, Silent Station,
//! their trials and their re-runs were retired by the owner on 2026-10-30 (ADR 0135, amended):
//! what they left in the store is kept and read past, never as a world.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, NewTask, ScenarioAction, TeamStore, WorkerAdapter};
use crate::domain::chess::Status;
use crate::domain::chess_match::{self, Match, Side};
use crate::domain::closure::{Closure, DoneWhy, DropWhy};
use crate::domain::pgn;
use crate::domain::{
    EventKind, MemberId, Priority, ProjectId, Refusal, RunId, TaskId, TaskStatus, WakeReason,
};

/// The tool's name, which a scenario's project lists alone.
pub const TOOL: &str = "scenario";

/// The longest action, as written, the log keeps.
const ACTION_MOST: usize = 400;

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

/// Where a task's place in a scenario is kept: `chess:<seed>:<black's task>` for a match;
/// `crew:<match>:<side>` for black's part. A retired world's word (a bare seed for a lighthouse,
/// `silent_station` or `silent_station_plan` for a mission) is kept as it was, and read as no
/// world.
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

/// Where a task stands in a match.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    /// The task whose world and log it is: the match, white's own.
    world: TaskId,
    /// Which game among the league's (chess has no chance in it).
    seed: u64,
    /// The world's parts: black's.
    parts: Vec<TaskId>,
    /// The side this task's member plays.
    role: Option<&'static str>,
}

/// Whose move it is, while the match goes on.
fn to_move(game: &Match) -> Option<&'static str> {
    game.outcome().is_none().then(|| game.to_move().name())
}

/// How a match ended: its name, and that it is the work done (playing it out was the work).
fn end(game: &Match) -> Option<(&'static str, bool)> {
    game.outcome().map(|(result, _)| (result, true))
}

/// Time spent in a match: its half-moves.
fn elapsed(game: &Match) -> u32 {
    u32::try_from(game.moves().len()).unwrap_or(u32::MAX)
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

    /// The owner starts a chess match between two different members, white and black, in
    /// `project`, which must give its work the scenario tool and nothing else: the match is
    /// white's task, and black has a part of it. `seed` names the game. Black's first turn
    /// finds it is white's move and ends; each legal move wakes the other side.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project or member there is not; [`Refusal::Scenario`] for
    /// a project that does not list the scenario tool alone, or one member on both sides; what
    /// adding the task refuses.
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

    /// Where a task stands in a match, if it is in one. A retired world's task, or a part of
    /// one, is in none.
    fn place_of(&self, task: TaskId) -> Done<Option<Place>> {
        let Some(kept) = self.store.setting(&key(task))? else {
            return Ok(None);
        };
        let words: Vec<&str> = kept.split(':').collect();
        let place = match words.as_slice() {
            [kind, seed, parts @ ..] if *kind == chess_match::CHESS => {
                seed.parse().ok().map(|seed| Place {
                    world: task,
                    seed,
                    parts: parts
                        .iter()
                        .filter_map(|p| p.parse().ok().map(TaskId))
                        .collect(),
                    role: Some(Side::White.name()),
                })
            }
            ["crew", game, side] => {
                let Some(game) = game.parse().ok().map(TaskId) else {
                    return Ok(None);
                };
                let Some(at) = self.place_of(game)?.filter(|p| p.world == game) else {
                    return Ok(None);
                };
                Some(Place {
                    role: Side::from_name(side).map(Side::name),
                    ..at
                })
            }
            _ => None,
        };
        Ok(place)
    }

    /// A task's scenario, by its seed, if it is in one.
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
        Ok(self.place_of(task)?.map(|_| chess_match::CHESS))
    }

    /// The member who plays a scenario's task: its assignee, or its owner.
    fn player_of(&self, task: TaskId) -> Done<Option<MemberId>> {
        Ok(self.store.task(task)?.and_then(|t| t.assignee.or(t.owner)))
    }

    /// Whether a task is in a scenario, a retired world's included: its world wakes its
    /// players, so the stuck-work guards and the ladder leave it alone.
    pub(super) fn in_a_scenario(&self, task: TaskId) -> Done<bool> {
        Ok(self.store.setting(&key(task))?.is_some())
    }

    /// Whether a match is being played: one at a time, for the league.
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

    /// What the tick does for the matches: one that has ended with its tasks still open (an
    /// end a rule added after its last move gives it) is closed; and one whose players have no
    /// turn coming, and that has sat idle, has them woken again, three times at most, and is
    /// then abandoned. Live, a turn stopped for its time had folded into it the wake that would
    /// have brought its player back, and a match waited for ever.
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
            if let Some((name, done)) = end(&now) {
                self.scenario_ended(&place, name, done, &now.look())?;
                continue;
            }
            // A paused world waits for the owner, and is not idle (baseline 28 Oct: a paused
            // league game was forfeited by the side whose wakes the pause held).
            let project = self.store.task(place.world)?.and_then(|t| t.project);
            if self.held_by_owner(project)? {
                continue;
            }
            self.wake_the_idle(&place, &now, &runs)?;
        }
        Ok(())
    }

    /// Wakes a match's side to move again when neither has a turn coming and it has sat idle
    /// since its last move or its last such wake; after [`REWAKES`] wakes with nothing done,
    /// abandons it. Kept as a setting a world: `<actions logged>:<wakes>:<when>`.
    fn wake_the_idle(
        &self,
        place: &Place,
        now: &Match,
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
            if self.forfeited(place, now, runs)? {
                return Ok(());
            }
            self.scenario_ended(
                place,
                "abandoned",
                false,
                &format!("nobody acted in it after {REWAKES} wakes"),
            )?;
            return Ok(());
        }
        // Who is to act: the side to move; otherwise every player still at it.
        let to_wake: Vec<TaskId> = match to_move(now) {
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

    /// A match as its log replays: its moves, and a side's resigning or forfeit.
    fn world_at(&self, place: &Place) -> Done<Match> {
        Match::replay_logged(&self.accepted(place.world)?).map_err(refused)
    }

    /// A chess match as its log replays.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task in no match; [`Refusal::Scenario`] for a log that
    /// does not replay, which an append-only log written by this engine never is.
    pub fn replay_chess(&self, task: TaskId) -> Done<Match> {
        let place = self
            .place_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        self.world_at(&place)
    }

    /// Who plays which side in a match: the holder of its own task and its part's member.
    fn roles_in(&self, place: &Place) -> Done<HashMap<MemberId, &'static str>> {
        let mut roles = HashMap::new();
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

    /// A match as the owner's screen shows it: its seed, the board, how it ended and what it
    /// scored; and its log move by move, illegal moves too, each with who played it, as which
    /// side, and what the world said.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no match (a retired world's among them); a
    /// store failure.
    pub fn scenario_shown(&self, task: TaskId) -> Done<Value> {
        let place = self
            .place_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let game = self.world_at(&place)?;
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
        let mut shown = json!({ "task": place.world.0, "kind": chess_match::CHESS,
                                "seed": place.seed, "end": end(&game).map(|(name, _)| name),
                                "elapsed": elapsed(&game), "said": game.look(),
                                "status": self.task(place.world)?.status.name(),
                                "log": log });
        shown["score"] = json!(game.score());
        shown["result"] = json!(game.outcome().map(|(r, _)| r));
        shown["reason"] = json!(game.outcome().map(|(_, why)| why));
        shown["moves"] = json!(game.move_text());
        shown["move_list"] = json!(game.moves());
        shown["positions"] = json!(game.positions());
        shown["to_move"] = json!(to_move(&game));
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
        shown["by_role"] = json!(self.by_role(&place, &roles)?);
        Ok(shown)
    }

    /// A match written out in PGN: a league game under the league, its sides the brains'
    /// labels; any other under the Holodeck, its sides the members playing. Dated by its first
    /// action; a match stopped with no result is abandoned.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task that is no match; a store failure.
    pub fn scenario_pgn(&self, task: TaskId) -> Done<String> {
        let place = self
            .place_of(task)?
            .ok_or(EngineError::Unknown("scenario"))?;
        let game = self.world_at(&place)?;
        let first = self
            .store
            .scenario_actions(place.world)?
            .first()
            .map(|a| a.at);
        let (event, round, white, black) = match self.league_game_of(place.world)? {
            Some((round, white, black)) => (
                pgn::LEAGUE,
                Some(round),
                white.label().to_owned(),
                black.label().to_owned(),
            ),
            None => {
                let names = self.names()?;
                let roles = self.roles_in(&place)?;
                let playing = |side: &str| {
                    roles
                        .iter()
                        .find(|(_, r)| **r == side)
                        .and_then(|(m, _)| names.get(m).cloned())
                        .unwrap_or_else(|| "?".to_owned())
                };
                (pgn::HOLODECK, None, playing("white"), playing("black"))
            }
        };
        let (status, reason) = game.outcome().map_or((Status::InProgress, ""), |(r, why)| {
            (Status::from_name(r).unwrap_or(Status::InProgress), why)
        });
        let abandoned = status == Status::InProgress && self.task(place.world)?.status.finished();
        Ok(pgn::pgn(&pgn::Record {
            event,
            date: first,
            round,
            white: &white,
            black: &black,
            moves: game.moves(),
            status,
            reason,
            abandoned,
        }))
    }

    /// Each side's moves in a match, legal and illegal, and who played it, for the owner and
    /// for comparing brains.
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

    /// The matches as the owner's screen lists them, newest first, the latest `most`: each
    /// one's task, kind, title and status, how it ended and its half-moves. Read from the
    /// projects that give their work the scenario tool alone, where every match is; a retired
    /// world's task there is passed over.
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
        let set_aside = self.league_games_set_aside()?;
        let mut listed = Vec::new();
        for task in tasks {
            if listed.len() >= most {
                break;
            }
            let Some(place) = self.place_of(task.id)?.filter(|p| p.world == task.id) else {
                continue;
            };
            let game = self.world_at(&place)?;
            listed.push(
                json!({ "task": task.id.0, "kind": chess_match::CHESS, "title": task.title,
                        "status": task.status.name(),
                        "end": end(&game).map(|(name, _)| name),
                        "elapsed": elapsed(&game),
                        "round": set_aside.contains(&task.id)
                            .then_some("before the league reset") }),
            );
        }
        Ok(listed)
    }

    /// The Holodeck as the owner's screen lists it: the latest matches, newest first, and the
    /// model league.
    pub fn holodeck_shown(&self) -> Done<Value> {
        Ok(json!({ "scenarios": self.scenarios_listed(20)?, "league": self.league_shown()? }))
    }

    /// Why a turn is refused one of HQ's tools on a match, if it is: moving a match's task, or
    /// black's part of it, is the world's, never a member's word.
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

    /// `scenario`: `look` says where the match stands; `move` and `resign` are the world's to
    /// accept or refuse, as the turn's side. Every move tried is kept in the world's log, illegal ones
    /// too, and said as an event; a legal one wakes the other side, and one that ends the match
    /// closes its tasks.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::Scenario`] for a turn whose task is no match, an
    /// action the world does not know, or a move it refuses; a store failure.
    pub fn scenario_in(&self, run: RunId, action: &str, san: Option<&str>) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let no_scenario = || refused("this turn's task is no scenario: there is nothing to act in");
        let task = turn.task.ok_or_else(no_scenario)?;
        let place = self.place_of(task)?.ok_or_else(no_scenario)?;
        // A side is its task's member's: anyone else woken on the task does not play it.
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
            if to_move(&now).is_some_and(|side| Some(side) != place.role) {
                return Ok(format!(
                    "{seen}\nIt is not your move: this turn has ended, and you are woken when \
                     it is."
                ));
            }
            return Ok(seen);
        }
        let written = match san {
            Some(san) if action.trim() == "move" => format!("move {}", san.trim()),
            // A side's resigning is kept with its side, so the log replays to who resigned.
            _ if action.trim() == "resign" => {
                format!("resign {}", place.role.unwrap_or_default())
            }
            _ => action.trim().to_owned(),
        };
        let kept: String = written.chars().take(ACTION_MOST).collect();
        // What is kept is what is played, so the log replays to the same world.
        let (next, said) = match Self::step(&now, place.role, &kept) {
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
        if let Some((name, done)) = end(&next) {
            self.scenario_ended(&place, name, done, &said)?;
            // The worker ends a turn HQ says has ended: live, models went on to set_status.
            return Ok(format!(
                "{said} The world has closed your task, and this turn has ended: stop now."
            ));
        }
        self.crew_told(&place, turn.member)?;
        // A match is a move a turn: the mover's turn ends, and the other side is woken.
        Ok(format!(
            "{said} Your move is made: this turn has ended, and you are woken when it is \
             your move again."
        ))
    }

    /// One move or a resignation, as written, by the member playing `side`.
    fn step(now: &Match, side: Option<&str>, written: &str) -> Result<(Match, String), String> {
        let side = side
            .and_then(Side::from_name)
            .ok_or("this task plays no side in the match")?;
        // Tipping the king: a side resigns, and the other wins.
        if written == format!("resign {}", side.name()) {
            return now.concede(side, &format!("{} resigned", side.name()));
        }
        let san = written
            .strip_prefix("move ")
            .filter(|s| !s.trim().is_empty())
            .ok_or("`action` is look, move with san (the move in standard notation), or resign")?;
        now.step(side, san)
    }

    /// A match whose side to move has stopped moving: that side forfeits, kept in the log as
    /// its own action on its last turn, and the other side wins (owner, 2026-10-28: else
    /// stopping is a way out of a lost game). False for a side that never had a turn to
    /// forfeit in.
    fn forfeited(
        &self,
        place: &Place,
        game: &Match,
        runs: &[crate::application::ports::Run],
    ) -> Done<bool> {
        let Some(side) = to_move(game) else {
            return Ok(false);
        };
        let tasks: Vec<TaskId> = std::iter::once(place.world)
            .chain(place.parts.iter().copied())
            .collect();
        let mut theirs = None;
        for task in tasks {
            if self.place_of(task)?.and_then(|p| p.role) == Some(side) {
                theirs = Some(task);
            }
        }
        let last = runs
            .iter()
            .filter(|r| theirs.is_some() && r.task == theirs)
            .max_by_key(|r| r.id.0);
        let (Some(last), Some(loser)) = (last, Side::from_name(side)) else {
            return Ok(false);
        };
        let (next, said) = game
            .concede(loser, &format!("{side} stopped moving, and forfeits"))
            .map_err(refused)?;
        self.act_kept(
            place,
            last.id,
            last.member,
            &format!("forfeit {side}"),
            true,
            &said,
            &next,
        )?;
        let (name, done) = end(&next).unwrap_or(("abandoned", false));
        self.scenario_ended(place, name, done, &said)?;
        Ok(true)
    }

    /// A match's other side is woken on its own task, to act on what changed.
    fn crew_told(&self, place: &Place, actor: MemberId) -> Done<()> {
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
        after: &Match,
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
            json!({ "task": place.world.0, "kind": chess_match::CHESS, "run": run.0,
                    "member": member.0, "by": by, "role": place.role,
                    "action": action, "ok": ok, "said": said, "elapsed": elapsed(after),
                    "end": end(after).map(|(name, _)| name) }),
        )
    }

    /// The world closes its tasks: done when it ended well, dropped (it cannot go on)
    /// otherwise. Black's part closes with it, after it, so nothing wakes white.
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
