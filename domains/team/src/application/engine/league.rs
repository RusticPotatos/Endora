//! The model league (ADR 0135, amended 2026-10-25): the owner names the brains and a project
//! that holds scenarios; HQ then sets chess games between them in the Holodeck, one at a time
//! and only while nothing else is being played and two members are free, each side pinned to
//! its brain whoever sits at the board. A Claude side is paid from the league's project, whose
//! cap bounds what the league spends. The table is rated from the record by
//! [`crate::domain::league`].

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::chess_match::Match;
use crate::domain::games::{self, Rng};
use crate::domain::league::{self, Played};
use crate::domain::{Brain, MemberId, Millis, ProjectId, Refusal, TaskId};
use crate::domain::{dates, money};

/// Where the league is kept: its project and its brains, by name.
const LEAGUE: &str = "league";
/// Where its games are kept, in the order they started: `[task, white, black]`.
const GAMES: &str = "league_games";

/// Where the games each reset set aside are kept: `[{at, games: [[task, white, black]]}]`.
const BEFORE_RESET: &str = "league_games_before_reset";

/// A league game as kept: its world's task and the brains playing white and black.
type Kept = (TaskId, Brain, Brain);

/// Where the brain a league game's side is pinned to is kept, by the side's task.
fn side_key(task: TaskId) -> String {
    format!("league_side:{}", task.0)
}

fn refused(why: impl Into<String>) -> EngineError {
    EngineError::Refused(Refusal::Scenario(why.into()))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The owner starts the league (or changes it): `brains` by name, two or more and each
    /// once, in `project`, which holds scenarios and, when a Claude plays, is capped. The games
    /// already played are kept.
    ///
    /// # Errors
    /// [`Refusal::Scenario`] for fewer than two brains, one there is not or named twice, a
    /// project that does not hold scenarios, or a Claude in an uncapped project; a store
    /// failure.
    pub fn start_league(&self, project: ProjectId, brains: &[&str]) -> Done<()> {
        let _held = self.held();
        let mut named: Vec<Brain> = Vec::new();
        for name in brains {
            let brain = Brain::from_name(name)
                .ok_or_else(|| refused(format!("there is no brain {name:?}")))?;
            if named.contains(&brain) {
                return Err(refused(format!("{name} is named twice")));
            }
            named.push(brain);
        }
        if named.len() < 2 {
            return Err(refused("a league is two brains or more"));
        }
        let found = self.holds_scenarios(project)?;
        if named.iter().any(|b| b.cloud()) && found.cap.is_none() {
            return Err(refused(format!(
                "Claude plays only from a capped project: cap project \"{}\" first",
                found.title
            )));
        }
        let names: Vec<&str> = named.iter().map(|b| b.name()).collect();
        self.store.set_setting(
            LEAGUE,
            &json!({ "project": project.0, "brains": names }).to_string(),
        )?;
        Ok(())
    }

    /// The league's project and brains, if the owner has started it.
    fn league(&self) -> Done<Option<(ProjectId, Vec<Brain>)>> {
        let Some(kept) = self
            .store
            .setting(LEAGUE)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        else {
            return Ok(None);
        };
        let brains = kept["brains"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| b.as_str().and_then(Brain::from_name))
                    .collect()
            })
            .unwrap_or_default();
        Ok(kept["project"].as_i64().map(|p| (ProjectId(p), brains)))
    }

    /// The league's games as kept: each game's task and its two brains.
    fn league_kept(&self) -> Done<Vec<(TaskId, Brain, Brain)>> {
        let kept: Vec<(i64, String, String)> = self.setting_json(GAMES)?.unwrap_or_default();
        Ok(kept
            .into_iter()
            .filter_map(|(task, w, b)| {
                Some((TaskId(task), Brain::from_name(&w)?, Brain::from_name(&b)?))
            })
            .collect())
    }

    /// The league's games with their results: a game whose world ended by the rules is scored;
    /// one still being played, or abandoned with no result, is not. Each with its match as its
    /// log replays.
    fn league_played(&self) -> Done<Vec<(TaskId, Played, Match)>> {
        self.league_kept()?
            .into_iter()
            .map(|(task, white, black)| self.league_game(task, white, black))
            .collect()
    }

    /// One league game, scored when its world ended by the rules, and its match.
    fn league_game(
        &self,
        task: TaskId,
        white: Brain,
        black: Brain,
    ) -> Done<(TaskId, Played, Match)> {
        let now = self.replay_chess(task)?;
        let score = now.outcome().map(|_| now.score());
        Ok((
            task,
            Played {
                white,
                black,
                score,
            },
            now,
        ))
    }

    /// The games each reset set aside, in the order kept: one list a round, the first round
    /// first, each with when it was set aside.
    fn league_rounds_before(&self) -> Done<Vec<(Millis, Vec<Kept>)>> {
        let before: Vec<Value> = self.setting_json(BEFORE_RESET)?.unwrap_or_default();
        Ok(before
            .iter()
            .map(|r| {
                let games = r["games"]
                    .as_array()
                    .map(|all| {
                        all.iter()
                            .filter_map(|g| {
                                Some((
                                    TaskId(g[0].as_i64()?),
                                    Brain::from_name(g[1].as_str()?)?,
                                    Brain::from_name(g[2].as_str()?)?,
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (r["at"].as_i64().unwrap_or(0), games)
            })
            .collect())
    }

    /// `at` as the owner's day says it: "17 Oct".
    fn owners_day(&self, at: Millis) -> String {
        let (day, month) = dates::day_and_month(money::day_of(at, self.settings.offset_minutes));
        format!("{day} {month}")
    }

    /// A league game's round (its place among the games kept, from 1) and the brains that
    /// played white and black, if the match on `world` is one.
    pub(super) fn league_game_of(&self, world: TaskId) -> Done<Option<(usize, Brain, Brain)>> {
        Ok(self
            .league_kept()?
            .into_iter()
            .enumerate()
            .find(|(_, (task, _, _))| *task == world)
            .map(|(i, (_, white, black))| (i + 1, white, black)))
    }

    /// The brain a turn on `task` is pinned to, if the task is a side of a league game.
    pub(super) fn league_brain_of(&self, task: Option<TaskId>) -> Done<Option<Brain>> {
        let Some(task) = task else {
            return Ok(None);
        };
        Ok(self
            .store
            .setting(&side_key(task))?
            .and_then(|name| Brain::from_name(&name)))
    }

    /// The owner resets the league: its games are set aside under
    /// [`BEFORE_RESET`], kept and never deleted, so every brain is rated afresh and every pair
    /// plays again; a game still being played stops, dropped. The brains and the project stay.
    /// Returns how many games were set aside and how many stopped.
    ///
    /// # Errors
    /// [`Refusal::Scenario`] when no league was started; a store failure.
    pub fn reset_league(&self) -> Done<Value> {
        let live: Vec<TaskId> = {
            let _held = self.held();
            if self.league()?.is_none() {
                return Err(refused("no league has been started"));
            }
            let kept = self.league_kept()?;
            let mut before: Vec<Value> = self.setting_json(BEFORE_RESET)?.unwrap_or_default();
            before.push(json!({
                "at": self.clock.now(),
                "games": kept.iter().map(|(t, w, b)| json!([t.0, w.name(), b.name()]))
                    .collect::<Vec<_>>(),
            }));
            self.store
                .set_setting(BEFORE_RESET, &Value::Array(before).to_string())?;
            self.store.set_setting(GAMES, "[]")?;
            let tasks = self.store.tasks()?;
            let live: Vec<TaskId> = kept
                .iter()
                .flat_map(|(game, _, _)| {
                    tasks.iter().filter(move |t| {
                        (t.id == *game || t.parent == Some(*game)) && !t.status.finished()
                    })
                })
                .map(|t| t.id)
                .collect();
            self.emit(
                crate::domain::EventKind::WorldChanged,
                json!({ "what": "league reset", "set_aside": kept.len(), "by": "owner" }),
            )?;
            live
        };
        for task in &live {
            self.drop_task(*task)?;
        }
        Ok(json!({ "set_aside": self.league_set_aside()?, "stopped": live.len() }))
    }

    /// How many games the last reset set aside.
    fn league_set_aside(&self) -> Done<usize> {
        let before: Vec<Value> = self.setting_json(BEFORE_RESET)?.unwrap_or_default();
        Ok(before
            .last()
            .and_then(|r| r["games"].as_array())
            .map_or(0, Vec::len))
    }

    /// The league games any reset set aside, by their world's task, so the owner's list can
    /// show them as from before it.
    pub(super) fn league_games_set_aside(&self) -> Done<Vec<TaskId>> {
        let before: Vec<Value> = self.setting_json(BEFORE_RESET)?.unwrap_or_default();
        Ok(before
            .iter()
            .filter_map(|r| r["games"].as_array())
            .flatten()
            .filter_map(|g| g[0].as_i64())
            .map(TaskId)
            .collect())
    }

    /// On the tick: when the league is on, no match is being played and two members are free,
    /// the next pair of brains sits down to a game, by two of the free members (who plays is
    /// the board's; which brain thinks is the league's).
    pub(super) fn league_tick(&self, now: Millis) -> Done<()> {
        let Some((project, brains)) = self.league()? else {
            return Ok(());
        };
        // Paused by the owner, it sets nobody down (baseline 28 Oct: it went on opening games).
        if self.held_by_owner(Some(project))? {
            return Ok(());
        }
        if self.a_world_is_live()? {
            return Ok(());
        }
        let mut kept = self.league_kept()?;
        let played: Vec<Played> = self
            .league_played()?
            .into_iter()
            .map(|(_, p, _)| p)
            .collect();
        let Some((white, black)) = league::next(&brains, &played) else {
            return Ok(());
        };
        let free = self.free_to_play(now)?;
        let ids: Vec<MemberId> = free.iter().map(|m| m.id).collect();
        let mut pick = Rng::new(games::seed(now, MemberId(0), MemberId(0), kept.len()));
        let Some(picked) = games::pick_two(&ids, &mut pick) else {
            return Ok(());
        };
        // Any member may play either side: a member is the model it is pinned to for the game.
        let (a, b) = picked;
        let seed = u64::try_from(kept.len()).unwrap_or(0) + 1;
        let game = self.start_chess_held_lock(project, [a, b], seed)?;
        let part = self
            .store
            .tasks()?
            .into_iter()
            .find(|t| t.parent == Some(game))
            .map(|t| t.id);
        self.store.set_setting(&side_key(game), white.name())?;
        if let Some(part) = part {
            self.store.set_setting(&side_key(part), black.name())?;
        }
        kept.push((game, white, black));
        let rows: Vec<(i64, &str, &str)> = kept
            .iter()
            .map(|(t, w, b)| (t.0, w.name(), b.name()))
            .collect();
        self.store.set_setting(GAMES, &json!(rows).to_string())?;
        Ok(())
    }

    /// The league for the screen: its brains in the standings' order (points, then rating), how
    /// many pairs have yet to finish a game, which round it is, and its games, latest last; a
    /// game in play with its board, the board before the last move, whose move it is and the
    /// last move (the owner's Chess tab, 31 Oct). `null` before the owner starts it.
    pub(super) fn league_shown(&self) -> Done<Value> {
        let Some((project, brains)) = self.league()? else {
            return Ok(Value::Null);
        };
        let played = self.league_played()?;
        let results: Vec<Played> = played.iter().map(|(_, p, _)| *p).collect();
        let table: Vec<Value> = league::table(&brains, &results)
            .into_iter()
            .map(|r| {
                json!({ "brain": r.brain.name(), "label": r.brain.label(), "rating": r.rating,
                        "points": r.points(), "won": r.won, "drawn": r.drawn, "lost": r.lost,
                        "history": r.history })
            })
            .collect();
        let mut games = Vec::new();
        for (task, p, now) in &played {
            let end = now.outcome().map(|(name, _)| name);
            let mut game = json!({ "task": task.0, "white": p.white.name(),
                                   "black": p.black.name(), "end": end,
                                   "half_moves": now.moves().len() });
            let live = end.is_none() && !self.task(*task)?.status.finished();
            game["live"] = json!(live);
            if live {
                let mut seen = now.positions();
                let rows = seen.pop();
                game["rows"] = json!(rows);
                if let Some((fen, _)) = now.board() {
                    game["fen"] = json!(fen);
                }
                game["before"] = json!(seen.pop().or_else(|| rows.clone()));
                game["to_move"] = json!(now.to_move().name());
                game["last"] = json!(now.moves().last());
            }
            games.push(game);
        }
        let before = self.league_rounds_before()?;
        let since = before.last().map(|(at, _)| self.owners_day(*at));
        Ok(
            json!({ "project": project.0, "table": table, "games": games,
                   "pairs_left": league::pairs_left(&brains, &results),
                   "round": { "number": before.len() + 1, "since": since } }),
        )
    }

    /// Every finished league game, of every round, newest first, `per` to a page (`page` from
    /// 1): each with its round, its brains, its result (or `abandoned` for one stopped with no
    /// result), its half-moves and the owner's day it ended. A game in play is not listed.
    ///
    /// # Errors
    /// A store failure.
    pub fn league_games(&self, page: usize, per: usize) -> Done<Value> {
        let per = per.max(1);
        let mut rounds = self.league_rounds_before()?;
        rounds.push((0, self.league_kept()?));
        let mut finished = Vec::new();
        for (round, (_, games)) in rounds.into_iter().enumerate() {
            for (task, white, black) in games {
                let (_, _, now) = self.league_game(task, white, black)?;
                let result = now.outcome().map(|(name, _)| name);
                if result.is_none() && !self.task(task)?.status.finished() {
                    continue;
                }
                finished.push((round + 1, task, white, black, result, now.moves().len()));
            }
        }
        finished.sort_by_key(|(_, task, ..)| std::cmp::Reverse(*task));
        let total = finished.len();
        let mut listed = Vec::new();
        for (round, task, white, black, result, half_moves) in finished
            .into_iter()
            .skip(page.saturating_sub(1).saturating_mul(per))
            .take(per)
        {
            let last = self.store.scenario_actions(task)?.last().map(|a| a.at);
            listed.push(
                json!({ "task": task.0, "round": round, "white": white.name(),
                                "white_label": white.label(), "black": black.name(),
                                "black_label": black.label(),
                                "result": result.unwrap_or("abandoned"),
                                "half_moves": half_moves,
                                "day": last.map(|at| self.owners_day(at)) }),
            );
        }
        Ok(
            json!({ "page": page.max(1), "pages": total.div_ceil(per), "per_page": per,
                   "total": total, "games": listed }),
        )
    }
}
