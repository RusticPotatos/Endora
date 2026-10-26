//! The model league (ADR 0135, amended 2026-10-25): the owner names the brains and a project
//! that holds scenarios; HQ then sets chess games between them in the Holodeck, one at a time
//! and only while nothing else is being played and two members are free, each side pinned to
//! its brain whoever sits at the board. A Claude side is paid from the league's project, whose
//! cap bounds what the league spends. The table is rated from the record by
//! [`crate::domain::league`].

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::games::{self, Rng};
use crate::domain::league::{self, Played};
use crate::domain::{Brain, MemberId, Millis, ProjectId, Refusal, Role, TaskId};

/// Where the league is kept: its project and its brains, by name.
const LEAGUE: &str = "league";
/// Where its games are kept, in the order they started: `[task, white, black]`.
const GAMES: &str = "league_games";

/// Where the brain a league game's side is pinned to is kept, by the side's task.
fn side_key(task: TaskId) -> String {
    format!("league_side:{}", task.0)
}

/// Whether a member who plays (any role but the CEO's) may think with `brain`.
fn may_play(brain: Brain) -> bool {
    !brain.cloud()
        || [
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
        ]
        .into_iter()
        .any(|r| brain.allowed_for(r))
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
        // The relay lets a role think only with its own Claudes (ADR 0111), and the CEO never
        // sits at the board: a brain only the CEO may think with has nobody to play it.
        if let Some(only_the_ceo) = named.iter().find(|b| !may_play(**b)) {
            return Err(refused(format!(
                "{} is the CEO's alone (ADR 0111), and the CEO does not play",
                only_the_ceo.label()
            )));
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
        let kept: Vec<(i64, String, String)> = self
            .store
            .setting(GAMES)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(kept
            .into_iter()
            .filter_map(|(task, w, b)| {
                Some((TaskId(task), Brain::from_name(&w)?, Brain::from_name(&b)?))
            })
            .collect())
    }

    /// The league's games with their results: a game whose world ended by the rules is scored;
    /// one still being played, or abandoned with no result, is not.
    fn league_played(&self) -> Done<Vec<(TaskId, Played, Option<&'static str>)>> {
        let mut played = Vec::new();
        for (task, white, black) in self.league_kept()? {
            let now = self.replay_chess(task)?;
            let end = now.outcome().map(|(name, _)| name);
            played.push((
                task,
                Played {
                    white,
                    black,
                    score: end.map(|_| now.score()),
                },
                end,
            ));
        }
        Ok(played)
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

    /// On the tick: when the league is on, no world is being played, nothing waits to run
    /// again and two members are free, the next pair of brains sits down to a game, by two of
    /// the free members (who plays is the board's; which brain thinks is the league's).
    pub(super) fn league_tick(&self, now: Millis) -> Done<()> {
        let Some((project, brains)) = self.league()? else {
            return Ok(());
        };
        if !self.rerun_queue()?.is_empty() || self.a_world_is_live()? {
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
        // Each side is played by a member whose role may think with its brain: the two picked
        // if they fit, either way round, or else the first two free members who do.
        let fits = |w: MemberId, b: MemberId| {
            let role = |id: MemberId| free.iter().find(|m| m.id == id).map(|m| m.role);
            w != b
                && role(w).is_some_and(|r| white.allowed_for(r) || !white.cloud())
                && role(b).is_some_and(|r| black.allowed_for(r) || !black.cloud())
        };
        let seated = [picked, (picked.1, picked.0)]
            .into_iter()
            .chain(ids.iter().flat_map(|w| ids.iter().map(move |b| (*w, *b))))
            .find(|(w, b)| fits(*w, *b));
        let Some((a, b)) = seated else {
            return Ok(());
        };
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

    /// The league for the screen: its brains rated best first, how many pairs have yet to
    /// finish a game, and its games, latest last. `null` before the owner starts it.
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
                        "won": r.won, "drawn": r.drawn, "lost": r.lost })
            })
            .collect();
        let games: Vec<Value> = played
            .iter()
            .map(|(task, p, end)| {
                json!({ "task": task.0, "white": p.white.name(), "black": p.black.name(),
                        "end": end })
            })
            .collect();
        Ok(
            json!({ "project": project.0, "table": table, "games": games,
                   "pairs_left": league::pairs_left(&brains, &results) }),
        )
    }
}
