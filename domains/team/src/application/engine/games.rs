//! Games in the office (ADR 0121). Ping-pong and pool are played by the engine itself, in code,
//! between two members who are free: no wake, no turn, no model, no spend. Chess is played by
//! the members, one move a downtime turn, by the rules in [`crate::domain::chess`]. This
//! module is the engine's side of both: the schedule, the tool's actions, what a downtime
//! briefing says about a game, and what the snapshot shows.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::chess::{self, Game, Refused};
use crate::domain::games::{self, Kind, Record, Rng};
use crate::domain::{EventKind, Member, MemberId, Millis, Refusal, Role, RunId};

/// The most results the office shows as the latest.
const RECENT: usize = 5;
/// The most finished games of chess the snapshot keeps showing, beside those being played.
const FINISHED_SHOWN: usize = 3;

fn refused(why: Refused) -> EngineError {
    EngineError::Refused(Refusal::Chess(why))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- ping-pong and pool ---------------------------------------------------------------

    /// What each tick does for the games (the lock held): free members play the matches that
    /// are due, if the service allows it, and a game of chess nobody has moved in for a week
    /// is adjourned.
    pub(super) fn games_tick(&self, now: Millis) -> Done<()> {
        if self.settings.games {
            self.play_matches(now)?;
        }
        self.adjourn_chess(now)
    }

    /// The members who are free to play: at work, not the CEO, with no turn running or
    /// queued, no open task and no routine due.
    fn free_to_play(&self, now: Millis) -> Done<Vec<Member>> {
        let tasks = self.store.tasks()?;
        let runs = self.store.runs()?;
        let routines = self.store.routines()?;
        let wakes = self.store.wakes()?;
        Ok(self
            .store
            .members()?
            .into_iter()
            .filter(|m| m.role != Role::Ceo && m.at_work())
            .filter(|m| {
                // A wake held behind another member's turn is work coming (ADR 0120): not idle.
                let busy = runs.iter().any(|r| r.member == m.id && r.status.live())
                    || wakes
                        .iter()
                        .any(|w| w.member == m.id && w.receipt == crate::domain::Receipt::Held);
                let holds = tasks.iter().any(|t| {
                    !t.status.finished() && (t.owner == Some(m.id) || t.assignee == Some(m.id))
                });
                let due = routines
                    .iter()
                    .any(|r| r.enabled && r.assignee == m.id && r.next_due_at <= now);
                !(busy || holds || due)
            })
            .collect())
    }

    /// Plays the matches that are due: at most one of each game every twenty minutes, between
    /// two free members who have not just played. A match is played at once, in code, from a
    /// seed made of the clock and the players, so it is the same match every time it is
    /// replayed.
    fn play_matches(&self, now: Millis) -> Done<()> {
        let mut records = self.store.games()?;
        let free = self.free_to_play(now)?;
        for (n, kind) in Kind::ALL.into_iter().enumerate() {
            if !games::due(kind, &records, now) {
                continue;
            }
            let ready: Vec<&Member> = free
                .iter()
                .filter(|m| games::rested(m.id, &records, now))
                .collect();
            let ids: Vec<MemberId> = ready.iter().map(|m| m.id).collect();
            let mut pick = Rng::new(games::seed(
                now,
                MemberId(0),
                MemberId(n as i64 + 1),
                records.len(),
            ));
            let Some((a, b)) = games::pick_two(&ids, &mut pick) else {
                continue;
            };
            let name = |id: MemberId| {
                ready
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(String::new, |m| m.name.clone())
            };
            let (name_a, name_b) = (name(a), name(b));
            let mut rng = Rng::new(games::seed(now, a, b, records.len()));
            let skill = |m: MemberId| games::skill(m, kind, games::played_by(m, kind, &records));
            let played = match kind {
                Kind::PingPong => games::play_ping_pong(skill(a), skill(b), &mut rng),
                Kind::Pool => games::play_pool(skill(a), skill(b), (&name_a, &name_b), &mut rng),
            };
            let mut record = Record {
                id: 0,
                kind,
                a,
                b,
                score_a: played.a,
                score_b: played.b,
                winner: if played.a_won { a } else { b },
                note: played.note,
                at: now,
            };
            record.id = self.store.add_game(&record)?;
            let (winner, loser) = if played.a_won {
                (&name_a, &name_b)
            } else {
                (&name_b, &name_a)
            };
            let (won, lost) = record.scores();
            self.emit(
                EventKind::GamePlayed,
                json!({ "game": kind.name(), "id": record.id,
                        "winner": winner, "winner_id": record.winner.0,
                        "loser": loser, "loser_id": record.loser().0,
                        "score": [won, lost], "note": record.note,
                        "text": record.said(winner, loser) }),
            )?;
            records.push(record);
        }
        Ok(())
    }

    // ---- chess ----------------------------------------------------------------------------

    /// The game a member is playing now, if it is in one.
    fn chess_game_of(&self, member: MemberId) -> Done<Option<Game>> {
        Ok(self
            .store
            .chess_games()?
            .into_iter()
            .find(|g| g.live() && g.plays(member)))
    }

    pub(super) fn names(&self) -> Done<HashMap<MemberId, String>> {
        Ok(self
            .store
            .members()?
            .into_iter()
            .map(|m| (m.id, m.name))
            .collect())
    }

    /// The board for one of the players: who plays, whose move it is, the position drawn, its
    /// notation, the last moves and, when it is their move, what they may play.
    fn board_said(&self, game: &Game, me: MemberId) -> Done<String> {
        let names = self.names()?;
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        let Some(view) = game.view() else {
            return Err(refused(Refused::Over));
        };
        let mine = game.to_move() == me;
        let opponent = game.opponent(me).map(name).unwrap_or_default();
        let mut told = format!(
            "Chess, game {}: {} (white) against {} (black). {}{}\n{}\nFEN: {}\nMoves: {}\n",
            game.id,
            name(game.white),
            name(game.black),
            if mine {
                "Your move.".to_owned()
            } else {
                format!("Waiting for {opponent}.")
            },
            if view.check && mine {
                " You are in check."
            } else {
                ""
            },
            view.ascii,
            view.fen,
            if game.moves.is_empty() {
                "none yet".to_owned()
            } else {
                game.move_text(Some(chess::LAST_MOVES))
            },
        );
        if mine {
            told.push_str(&format!("Legal moves: {}\n", view.legal.join(", ")));
        }
        Ok(told)
    }

    /// `chess` / `board`: the member's game. Allowed in any turn.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refused::NoGame`] if it has no game; a store failure.
    pub fn chess_board_in(&self, run: RunId) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let Some(game) = self.chess_game_of(turn.member)? else {
            return Err(refused(Refused::NoGame));
        };
        self.board_said(&game, turn.member)
    }

    /// Marks the running turn as having changed something through HQ.
    fn chess_moved_forward(&self, run: RunId) -> Done<()> {
        if let Some(mut found) = self.store.run(run)? {
            found.moved_forward = true;
            self.store.save_run(&found)?;
        }
        Ok(())
    }

    /// `chess` / `move`: plays a move for the member in its game, if this is a downtime turn,
    /// it is the member's move and the move is legal. Refused otherwise, with the legal
    /// moves when it was the move that was wrong.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; the chess refusals; a store failure.
    pub fn chess_move_in(&self, run: RunId, san: &str) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        if !self.in_downtime(run)? {
            return Err(refused(Refused::NotInDowntime));
        }
        let Some(mut game) = self.chess_game_of(turn.member)? else {
            return Err(refused(Refused::NoGame));
        };
        let now = self.clock.now();
        let moved = game.play(turn.member, san, now).map_err(refused)?;
        self.store.save_chess_game(&game)?;
        let names = self.names()?;
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        let opponent = game.opponent(turn.member).map(name).unwrap_or_default();
        let me = name(turn.member);
        self.emit(
            EventKind::ChessMoved,
            json!({ "game": game.id, "by": me, "member": turn.member.0, "san": moved.san,
                    "move": game.moves.len().div_ceil(2),
                    "text": format!("{me} played {} against {opponent} at chess (move {})",
                                    moved.san, game.moves.len().div_ceil(2)) }),
        )?;
        self.chess_moved_forward(run)?;
        let Some((_, reason)) = moved.ended else {
            return Ok(format!(
                "You played {}. Waiting for {opponent}: they move in their next downtime turn.",
                moved.san
            ));
        };
        let result = chess::result_said(&game, &name(game.white), &name(game.black));
        self.emit_chess_ended(&game, &result)?;
        Ok(format!("You played {}: {reason}. {result}.", moved.san))
    }

    fn emit_chess_ended(&self, game: &Game, text: &str) -> Done<()> {
        self.emit(
            EventKind::ChessEnded,
            json!({ "game": game.id, "status": game.status.name(), "reason": game.reason,
                    "white": game.white.0, "black": game.black.0,
                    "winner": game.winner().map(|w| w.0), "text": text }),
        )
    }

    /// `chess` / `challenge`: starts a game with another member, who has black; the challenger
    /// has white and the first move. One game each, and only a member at hand can be asked.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; the chess refusals; a store failure.
    pub fn chess_challenge_in(&self, run: RunId, whom: &str) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        if !self.in_downtime(run)? {
            return Err(refused(Refused::NotInDowntime));
        }
        let me = self.member(turn.member)?;
        let said = whom.trim();
        let Some(other) = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(said))
        else {
            return Err(refused(Refused::NoSuchMember(said.to_owned())));
        };
        if other.id == me.id {
            return Err(refused(Refused::Yourself));
        }
        if !other.at_work() || other.role == Role::Ceo {
            return Err(refused(Refused::NotAvailable(other.name)));
        }
        if self.chess_game_of(me.id)?.is_some() {
            return Err(refused(Refused::InAGame(me.name)));
        }
        if self.chess_game_of(other.id)?.is_some() {
            return Err(refused(Refused::InAGame(other.name)));
        }
        let id = self
            .store
            .add_chess_game(me.id, other.id, self.clock.now())?;
        self.emit(
            EventKind::ChessStarted,
            json!({ "game": id, "white": me.id.0, "black": other.id.0,
                    "text": format!("{} challenges {} to chess", me.name, other.name) }),
        )?;
        self.chess_moved_forward(run)?;
        Ok(format!(
            "You challenged {} to chess (game {id}): you have white and the first move. Look at \
             the board with `board` and play with `move`; a move is one downtime turn, and \
             {} is told of the game in theirs.",
            other.name, other.name
        ))
    }

    /// What a downtime briefing says about chess: whose move it is in the member's game, or
    /// that it may challenge someone. Short.
    pub(super) fn chess_briefed(&self, member: &Member) -> Done<String> {
        let Some(game) = self.chess_game_of(member.id)? else {
            return Ok(
                "Chess: you have no game. You may challenge a member who is at hand with \
                       chess (action challenge); one game each, and a move is one downtime turn.\n"
                    .to_owned(),
            );
        };
        let names = self.names()?;
        let opponent = game
            .opponent(member.id)
            .and_then(|o| names.get(&o))
            .cloned()
            .unwrap_or_default();
        if game.to_move() == member.id {
            return Ok(format!(
                "Chess: Your move against {opponent} in chess (game {}). Look with chess (action \
                 board), then play one move (action move, san).\n",
                game.id
            ));
        }
        Ok(format!(
            "Chess: waiting for {opponent}'s move in your game ({}); chess (action board) shows \
             the position.\n",
            game.id
        ))
    }

    /// Adjourns the games of chess nobody has moved in for a week, as draws.
    fn adjourn_chess(&self, now: Millis) -> Done<()> {
        let mut stale: Vec<Game> = self
            .store
            .chess_games()?
            .into_iter()
            .filter(|g| g.stale(now))
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        let names = self.names()?;
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        for game in &mut stale {
            if !game.adjourn(now) {
                continue;
            }
            self.store.save_chess_game(game)?;
            let result = chess::result_said(game, &name(game.white), &name(game.black));
            self.emit_chess_ended(game, &result)?;
        }
        Ok(())
    }

    // ---- what the screen shows ------------------------------------------------------------

    fn game_shown(&self, game: &Game, names: &HashMap<MemberId, String>) -> Value {
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        let view = game.view();
        json!({
            "id": game.id,
            "white": name(game.white), "black": name(game.black),
            "white_id": game.white.0, "black_id": game.black.0,
            "status": game.status.name(), "reason": game.reason,
            "moves": game.moves, "move_text": game.move_text(None),
            "rows": view.as_ref().map(|v| v.rows.clone()).unwrap_or_default(),
            "fen": view.as_ref().map(|v| v.fen.clone()).unwrap_or_default(),
            "check": view.as_ref().is_some_and(|v| v.check),
            "to_move": if game.live() { Value::from(name(game.to_move())) } else { Value::Null },
            "started_at": game.started_at, "last_move_at": game.last_move_at,
            "result": if game.live() { String::new() }
                      else { chess::result_said(game, &name(game.white), &name(game.black)) },
        })
    }

    /// A kept match of ping-pong or pool played again, step by step, from what it was played
    /// from (its seed and the players' skills then): every rally or shot with the score after
    /// it, and whether the replay ends as the record does.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a match there is not; a store failure.
    pub fn game_replay(&self, id: i64) -> Done<Value> {
        let mut records = self.store.games()?;
        records.sort_by_key(|r| r.id);
        let at = records
            .iter()
            .position(|r| r.id == id)
            .ok_or(EngineError::Unknown("match"))?;
        let record = &records[at];
        let again = games::replay(record, &records[..at]);
        let names = self.names()?;
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        let player = |by: u8| {
            if by == 0 {
                name(record.a)
            } else {
                name(record.b)
            }
        };
        let steps: Vec<Value> = again
            .steps
            .iter()
            .map(|step| match *step {
                games::Step::Rally { a_won, a, b } => json!({ "step": "rally",
                    "won_by": player(u8::from(!a_won)), "score": [a, b] }),
                games::Step::Shot { by, potted, a, b } => json!({ "step": "shot",
                    "by": player(by), "potted": potted, "balls": [a, b] }),
                games::Step::Scratch { by } => json!({ "step": "scratch", "by": player(by) }),
            })
            .collect();
        Ok(json!({ "id": record.id, "kind": record.kind.name(),
                   "players": [name(record.a), name(record.b)],
                   "skills": [again.skills.0, again.skills.1], "seed": again.seed.to_string(),
                   "score": [record.score_a, record.score_b], "winner": name(record.winner),
                   "note": record.note, "at": record.at, "matches": again.matches,
                   "steps": steps }))
    }

    /// The games as the HQ screen shows them: the leaderboards, the latest results, and the
    /// games of chess being played and the last ones finished.
    pub(super) fn games_shown(&self) -> Done<Value> {
        let names = self.names()?;
        let name = |id: MemberId| names.get(&id).cloned().unwrap_or_default();
        let records = self.store.games()?;
        let chess_games = self.store.chess_games()?;
        let board = |kind: Kind| -> Value {
            let last = records.iter().rfind(|r| r.kind == kind).map(|r| {
                let (won, lost) = r.scores();
                json!({ "winner": name(r.winner), "loser": name(r.loser()),
                        "score": [won, lost], "note": r.note, "at": r.at })
            });
            json!({
                "label": kind.label(),
                "last": last,
                "leaderboard": games::standings(kind, &records).iter().map(|s| json!({
                    "member": s.member.0, "name": name(s.member),
                    "wins": s.wins, "losses": s.losses, "streak": s.streak,
                })).collect::<Vec<_>>(),
            })
        };
        let mut recent: Vec<(Millis, i64, Value)> = records
            .iter()
            .map(|r| {
                let text = r.said(&name(r.winner), &name(r.loser()));
                (
                    r.at,
                    r.id,
                    json!({ "id": r.id, "game": r.kind.name(), "text": text, "at": r.at }),
                )
            })
            .collect();
        recent.extend(chess_games.iter().filter(|g| !g.live()).map(|g| {
            let text = chess::result_said(g, &name(g.white), &name(g.black));
            (
                g.last_move_at,
                g.id,
                json!({ "id": g.id, "game": "chess", "text": text, "at": g.last_move_at }),
            )
        }));
        recent.sort_by_key(|(at, id, _)| std::cmp::Reverse((*at, *id)));
        let mut shown: Vec<&Game> = chess_games.iter().filter(|g| g.live()).collect();
        shown.extend(
            chess_games
                .iter()
                .rev()
                .filter(|g| !g.live())
                .take(FINISHED_SHOWN),
        );
        Ok(json!({
            "ping_pong": board(Kind::PingPong),
            "pool": board(Kind::Pool),
            "chess": {
                "label": "chess",
                "leaderboard": chess::standings(&chess_games).iter().map(|s| json!({
                    "member": s.member.0, "name": name(s.member),
                    "wins": s.wins, "losses": s.losses, "draws": s.draws,
                })).collect::<Vec<_>>(),
                "games": shown.iter().map(|g| self.game_shown(g, &names)).collect::<Vec<_>>(),
            },
            "recent": recent.into_iter().take(RECENT).map(|(.., v)| v).collect::<Vec<_>>(),
        }))
    }
}
