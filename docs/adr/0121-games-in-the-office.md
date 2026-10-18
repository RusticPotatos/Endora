# 0121 — Games in the office

## Status

Accepted, 2026-10-18, by the owner. Prepared with AI assistance (Claude). Extends
[0119](0119-the-open-office.md) (the office as data, downtime, the Office card). Built: the
simulated ping-pong and pool, the chess table and the `chess` tool, the `games` and
`chess_games` tables, the Games section of the Office card, and the owner's scenarios in
`domains/team/features/the-office.feature`.

## Context

The owner, on the ping-pong table that only ever stood there: "Cheap things too: make the ping
pong games actually keep track of points, would be funny. Maybe a pool table with an actual
game. Or chess with actual games. Fun to think about."

Two things are true of the office. Two idle members already stand at the ends of the ping-pong
table and a ball bounces between them, but nothing is played and nothing is kept. And a
downtime turn ([0119](0119-the-open-office.md)) is a model's turn: free on the house's model,
but a turn all the same, with a briefing, a container and a place in the queue. Keeping score
at ping-pong is not worth a turn, and a model is the worst referee for a rally.

## Decision

**Owner decisions, 2026-10-18.**

1. **Ping-pong and pool are played by HQ's engine, in code, for nothing.** No wake, no turn, no
   container, no model and no spend. When two members are free (at work, not the CEO, no turn
   queued or running, no open task, no routine due, and not in a match in the last ten minutes)
   the engine plays the match that is due, at once, point by point or shot by shot, and keeps
   it. A game is played at most once every twenty minutes, so at most three an hour, one at a
   time. A switch in `Settings` (`games`) turns it on; the service does, the scripted demo and
   the engine's older tests leave it off.
2. **The matches are deterministic.** A small generator in the domain (splitmix64; no new
   dependency) is seeded from the engine's clock, the two members' numbers and the number of
   matches so far, so the same team and clock replay the same matches. Each member has a hidden
   skill at each game, 35 to 65 per cent from its number, and a point more for every ten games
   of that kind it has played, to ten points. The chance of winning a rally is 50 per cent plus
   half the difference in skill, kept between 10 and 90.
3. **The rules are the real ones, a little simplified.** Ping-pong: first to 11, and two clear.
   Pool is eight-ball without fouls beyond one: each player has seven balls and then the eight;
   a player who pots goes on shooting, one who misses hands the table over; whoever pots the
   eight first wins; a miss at the eight is a scratch one time in five, and scratching on the
   eight loses. The score of a pool match is the balls each potted, the eight counting as the
   eighth.
4. **Every match is kept.** A `games` table: kind, the two players, the score, the winner, a
   note ("Dee scratched on the 8") and when. The feed says it ("Bo beat Dee 11–8 at ping-pong",
   `game.played`). The leaderboard per game (wins, losses and the run it is on) is read from the
   records alone and is in the snapshot with the latest match at each table and the last five
   results of all.
5. **Chess is real, and played by the members.** A `chess_games` table: the two players, the
   moves as standard notation, the status (in progress, white won, black won, drawn) and why,
   when it began and when it last moved. A team tool, `chess`, for every role but the CEO:
   `board` (the position as a FEN and drawn in plain text, whose move it is, the last moves and,
   on one's own move, the legal moves; allowed in any turn), `move` (one move in notation; played
   only if it is the member's move and legal, refused with the legal moves otherwise) and
   `challenge` (a member who is at hand, not in a game; the challenger has white). One game each.
   **Moves and challenges are made only in downtime turns**, which is where ADR 0119 puts every
   leisure act: free on the house's model, and for a paid member only within the owner's
   allowance. A downtime briefing says "Your move against Dee in chess" when it is the member's
   turn, and that it may challenge someone when it has no game. A game is one move a downtime
   turn, so it is slow; that is the owner's cheap fun. A game nobody moves in for seven days is
   adjourned as a draw.
6. **The rules of chess are a library's.** Legal move generation, check, checkmate and
   stalemate, the notation, and whether two positions are the same by the FIDE definition are
   `cozy-chess`'s; insufficient material (bare kings, a lone minor piece, bishops all on one
   colour), threefold repetition and the fifty-move rule (100 halfmoves) are decided in
   `domain/chess.rs` from the positions the game has been through. A game is stored as its
   moves and replayed to find the position, so there is no second copy of the board to
   disagree with the moves.
7. **The new dependency, and the one not taken.** `cozy-chess` 0.3 is added to `endora-team`
   only. It is pure Rust with no IO, MIT licensed, and exactly the kind of work not worth
   writing by hand and getting wrong: castling, en passant, promotion, pins and the draws. It
   has one small crate beneath it (`cozy-chess-types`, also MIT). The domain stays free of IO
   and of every vendor. **`shakmaty`, the better-known crate, was the first choice and was
   dropped: it is GPL-3.0-or-later, and Endora is Apache-2.0** (a binary that links a GPL crate
   is a GPL work). Choosing it would be the owner's decision to take with that cost in view,
   not an implementation detail; if the owner wants it, the swap is confined to
   `domain/chess.rs`.
8. **On the screen.** The office has a **pool table** (three tiles by two) at the lounge's far
   side and a **chess table** (one tile) in the lounge, as fixtures of the layout like the
   ping-pong table; the rules of the office reserve them and the row or tiles where their
   players stand, and the screen blocks the same tiles (a test on each side). Idle members drift
   to the tables' sides at random, and two at a pair play. When the engine plays a match the
   two members walk to that table, the winner says the score and the other "good game"; a
   scoreboard by the ping-pong and the pool table says the latest. The live game of chess is
   drawn on the chess table. Tapping a table opens a panel: the chess board (unicode pieces), the
   moves and whose move it is or the result; or the pool or ping-pong leaderboard. The Office
   card has a **Games** section: the three leaderboards, the last five results and the games of
   chess being played. Every name and every move is the team's own words and is escaped.

## Consequences

- Ping-pong and pool add no model cost at all, whatever the team's size, and no turn to the
  scorecard.
- Chess adds a few lines to a downtime briefing and one tool; a downtime turn that plays a move
  is a turn of the ordinary kind. Chess moves cost what the turn costs: nothing on the house's
  model.
- New data on the live store: two tables, made if missing when HQ opens the store. No row
  changes.
- One new dependency (above), with one small one under it, both MIT.
- The pool and chess tables are part of the layout the placement rules reserve, so the team
  cannot decorate over them and the default layout does not touch them. A thing the team had
  already put on those tiles stays where it is; it is not removed.
- Matches are seeded from the clock, so a replay of the engine on the same clock plays the same
  matches; a test pins a fixed seed's score.

## Deferred

- Chess clocks, a draw offered or resigned: a game ends by the rules or by the week.
- Playing at chess against the house's model at its own pace, outside downtime.
- Fouls in pool beyond the scratch on the eight; a break; calling pockets.
- A tournament, a season or a trophy.

## Alternatives considered

- **Model turns for ping-pong and pool.** Rejected: a turn for every rally is a cost for no
  gain, and a model is no referee. The match is a pure function of two skills and a seed.
- **Hand-written chess.** Rejected: castling, en passant, promotion, pins and the draws are
  where a hand-written engine is wrong in the corner nobody tests. The dependency is small and
  pure.
- **`shakmaty`.** Rejected on its licence (GPL-3.0-or-later against Endora's Apache-2.0), as
  above. `chess` 3.2 (MIT) was also looked at: it was last released in 2021;
  `cozy-chess` is newer and smaller, and writes standard notation.
- **Chess played outside downtime.** Rejected: every leisure act in the office is a downtime
  act ([0119](0119-the-open-office.md)), so nothing the team does for fun competes with work or
  spends the owner's money unasked.
- **Storing the board.** Rejected: the moves are the truth; the position is derived.
