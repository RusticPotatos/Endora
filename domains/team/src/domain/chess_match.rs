//! A chess match as a scenario (the Holodeck plan, after phase 7): two members, white and
//! black, play by the rules of [`super::chess`], a move a turn, and the world keeps every move
//! tried, the illegal ones too. Nothing is stored but the moves: the board is the moves
//! replayed. A match ends by the rules (mate, stalemate, repetition, too little material) or
//! at the move limit, as a draw, so a match between two models always ends.

use super::chess::{Game, Refused, Status};
use super::work::MemberId;

/// The match's kind, as a scenario names it.
pub const CHESS: &str = "chess";

/// The rules' version: a trial compares matches of one version alone.
pub const VERSION: u32 = 1;

/// The most half-moves a match is played to: then it is drawn.
pub const MOVE_LIMIT: usize = 60;

/// Who stands for white and for black in the world's own game: the members are kept outside it.
const WHITE: MemberId = MemberId(1);
const BLACK: MemberId = MemberId(2);

/// A side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Moves first.
    White,
    /// Moves second.
    Black,
}

impl Side {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Black => "black",
        }
    }

    /// From its name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "white" => Some(Self::White),
            "black" => Some(Self::Black),
            _ => None,
        }
    }

    const fn member(self) -> MemberId {
        match self {
            Self::White => WHITE,
            Self::Black => BLACK,
        }
    }
}

/// A match as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    game: Game,
}

impl Default for Match {
    fn default() -> Self {
        Self::new()
    }
}

impl Match {
    /// A new match, white to move.
    #[must_use]
    pub fn new() -> Self {
        Self {
            game: Game::new(0, WHITE, BLACK, 0),
        }
    }

    /// The moves played, in notation.
    #[must_use]
    pub fn moves(&self) -> &[String] {
        &self.game.moves
    }

    /// The side to move.
    #[must_use]
    pub fn to_move(&self) -> Side {
        if self.game.to_move() == WHITE {
            Side::White
        } else {
            Side::Black
        }
    }

    /// How it ended (`white_won`, `black_won`, `draw`) and why, if it has.
    #[must_use]
    pub fn outcome(&self) -> Option<(&'static str, &str)> {
        if self.game.live() {
            return None;
        }
        Some((self.game.status.name(), &self.game.reason))
    }

    /// What it scores, from white's side: 100 a white win, 50 a draw, 0 a black win.
    #[must_use]
    pub fn score(&self) -> u32 {
        match self.game.status {
            Status::WhiteWon => 100,
            Status::Draw => 50,
            Status::BlackWon | Status::InProgress => 0,
        }
    }

    /// One move by `side`, in standard notation: the match after it and what the world says,
    /// or why it is refused. A refused move changes nothing.
    ///
    /// # Errors
    /// The rules' refusal: the match is over, it is the other side's move, or the move is not
    /// legal (with the legal moves).
    pub fn step(&self, side: Side, san: &str) -> Result<(Self, String), String> {
        let mut next = self.clone();
        let moved = next
            .game
            .play(side.member(), san, 0)
            .map_err(|refused| match refused {
                Refused::NotYourTurn => format!(
                    "it is {}'s move, not {}'s",
                    self.to_move().name(),
                    side.name()
                ),
                other => other.to_string(),
            })?;
        if next.game.live() && next.game.moves.len() >= MOVE_LIMIT {
            next.game.status = Status::Draw;
            next.game.reason = format!("drawn at the move limit, {MOVE_LIMIT} half-moves");
        }
        let said = match next.outcome() {
            Some((result, why)) => format!(
                "{} played {}. The match is over: {result}, {why}.",
                side.name(),
                moved.san
            ),
            None => format!(
                "{} played {}. It is {}'s move.",
                side.name(),
                moved.san,
                next.to_move().name()
            ),
        };
        Ok((next, said))
    }

    /// The match with `moves` played in order: what a log replays to.
    ///
    /// # Errors
    /// The first move that is refused, which a kept log never holds.
    pub fn replay(moves: &[String]) -> Result<Self, String> {
        moves.iter().try_fold(Self::new(), |at, san| {
            let side = at.to_move();
            Ok(at.step(side, san)?.0)
        })
    }

    /// The board as a side sees it: the position, drawn, the last moves, whose move, and, for
    /// the side to move, the legal moves.
    #[must_use]
    pub fn look(&self, side: Option<Side>) -> String {
        let Some(view) = self.game.view() else {
            return "The board cannot be read.".to_owned();
        };
        let mut said = format!(
            "{}\nFEN {}\nMoves: {}\n",
            view.ascii.trim_end(),
            view.fen,
            if self.game.moves.is_empty() {
                "none yet".to_owned()
            } else {
                self.game.move_text(Some(super::chess::LAST_MOVES))
            }
        );
        if let Some((result, why)) = self.outcome() {
            said.push_str(&format!("The match is over: {result}, {why}."));
            return said;
        }
        let to_move = self.to_move();
        said.push_str(&format!(
            "{} to move{}; {} of {MOVE_LIMIT} half-moves played.",
            to_move.name(),
            if view.check { ", in check" } else { "" },
            self.game.moves.len()
        ));
        if side == Some(to_move) {
            said.push_str(&format!(" Your legal moves: {}.", view.legal.join(", ")));
        }
        said
    }

    /// The position in Forsyth-Edwards notation, and its rows for the screen.
    #[must_use]
    pub fn board(&self) -> Option<(String, Vec<String>)> {
        self.game.view().map(|v| (v.fen, v.rows))
    }

    /// The moves as written down: `1. e4 e5 2. Nf3`.
    #[must_use]
    pub fn move_text(&self) -> String {
        self.game.move_text(None)
    }
}

/// What a side is told before it plays: the match's description.
#[must_use]
pub fn brief(side: Side) -> String {
    format!(
        "A chess match. You play {} against a teammate, by the rules, a move a turn, with the \
         scenario tool: look shows the board, whose move it is and, on your move, your legal \
         moves; move plays one in standard notation (san, such as e4, Nf3, exd5, O-O). An \
         illegal move is refused and costs nothing, and is kept. After your move your turn \
         ends, and you are woken when it is your move again. The match ends by the rules or at \
         {MOVE_LIMIT} half-moves, as a draw; the world closes it.",
        side.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn played(moves: &[&str]) -> Match {
        Match::replay(&moves.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn sides_move_in_turn_and_a_refused_move_changes_nothing() {
        let start = Match::new();
        assert_eq!(
            start.step(Side::Black, "e5").unwrap_err(),
            "it is white's move, not black's"
        );
        let refused = start.step(Side::White, "e5").unwrap_err();
        assert!(
            refused.contains("e4"),
            "the legal moves come with it: {refused}"
        );
        let (after, said) = start.step(Side::White, "e4").unwrap();
        assert_eq!(said, "white played e4. It is black's move.");
        assert_eq!((after.to_move(), start.moves().len()), (Side::Black, 0));
        assert!(after.look(Some(Side::Black)).contains("Your legal moves:"));
        assert!(!after.look(Some(Side::White)).contains("Your legal moves:"));
    }

    #[test]
    fn mate_ends_it_by_the_rules_and_scores_from_whites_side() {
        let mate = played(&["f3", "e5", "g4"]);
        let (over, said) = mate.step(Side::Black, "Qh4").unwrap();
        assert_eq!(over.outcome().map(|(r, _)| r), Some("black_won"));
        assert!(said.contains("over: black_won"), "{said}");
        assert_eq!(over.score(), 0);
        assert!(over.step(Side::White, "a3").unwrap_err().contains("over"));
    }

    #[test]
    fn a_match_that_runs_to_the_move_limit_is_drawn() {
        // Knights out and back for ever: the rules draw it by repetition, or the limit does;
        // either way it ends, drawn.
        let mut at = Match::new();
        let shuffle = ["Nf3", "Nf6", "Ng1", "Ng8", "Nc3", "Nc6", "Nb1", "Nb8"];
        let mut i = 0;
        while at.outcome().is_none() {
            let side = at.to_move();
            at = at.step(side, shuffle[i % shuffle.len()]).unwrap().0;
            i += 1;
        }
        assert_eq!(at.outcome().map(|(r, _)| r), Some("draw"));
        assert!(at.moves().len() <= MOVE_LIMIT);
        assert_eq!(at.score(), 50);
    }

    #[test]
    fn the_moves_replay_to_the_same_match() {
        let a = played(&["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(Match::replay(a.moves()).unwrap(), a);
        assert_eq!(a.move_text(), "1. e4 e5 2. Nf3 Nc6");
        assert!(a.board().unwrap().0.starts_with("r1bqkbnr/pppp1ppp/2n5"));
        assert!(brief(Side::Black).contains("You play black"));
    }
}
