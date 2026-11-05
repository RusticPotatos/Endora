//! Chess between members (ADR 0121), by the rules. The legal moves, check, mate and stalemate,
//! the notation and repeated positions are `cozy-chess`'s, a pure library with no IO; this
//! module keeps a game as the list of moves played in standard notation, replays it to find the
//! position, and decides whose turn it is, whether a move may be played, and whether the game
//! is over: also by insufficient material, threefold repetition and the fifty-move rule, and by
//! a week without a move.

use core::fmt;

use cozy_chess::util::{display_san_move, parse_san_move};
use cozy_chess::{Board, Color, File, GameStatus, Piece, Rank, Square};

use super::work::{MemberId, Millis};

/// How long a game may wait for a move before it is adjourned, as a draw: seven days.
pub const ADJOURN_AFTER_MS: Millis = 7 * 24 * 60 * 60 * 1000;
/// The halfmoves without a capture or a pawn move after which the game is drawn.
const FIFTY_MOVES: u32 = 100;
/// How many of the last moves a board shows.
pub const LAST_MOVES: usize = 6;

/// Where a game stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Being played.
    InProgress,
    /// White won.
    WhiteWon,
    /// Black won.
    BlackWon,
    /// Drawn.
    Draw,
}

impl Status {
    /// The name as stored.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::WhiteWon => "white_won",
            Self::BlackWon => "black_won",
            Self::Draw => "draw",
        }
    }

    /// The status with this stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::InProgress, Self::WhiteWon, Self::BlackWon, Self::Draw]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// Why a move, a challenge or a look is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The game is over.
    Over,
    /// It is the other player's move.
    NotYourTurn,
    /// What was written is not a move in standard notation.
    NotAMove(String),
    /// A move in notation, but not legal here. As in standard play, the player is not told
    /// which moves are.
    Illegal {
        /// What was written.
        said: String,
    },
    /// The member is not in a game.
    NoGame,
    /// Chess moves are made in downtime turns, when a member has nothing else to do.
    NotInDowntime,
    /// A member is challenged who is already in a game of chess: one game each.
    InAGame(String),
    /// The member challenges itself.
    Yourself,
    /// The member challenged is not there to play (paused, on furlough, or the CEO).
    NotAvailable(String),
    /// Nobody of that name.
    NoSuchMember(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Over => f.write_str("that game is over"),
            Self::NotYourTurn => f.write_str("it is not your move: wait for your opponent"),
            Self::NotAMove(said) => write!(
                f,
                "`{said}` is not a move: write it in standard notation, such as e4, Nf3, O-O or exd5"
            ),
            Self::Illegal { said } => write!(f, "`{said}` is not legal here"),
            Self::NoGame => f.write_str(
                "you have no game of chess: challenge a member with `challenge` to start one",
            ),
            Self::NotInDowntime => f.write_str(
                "chess is played in a turn on the house's own brains (its model or the Mac), \
                 never in one on Claude: you may look at the board now, and move in your next \
                 turn on the house's brains",
            ),
            Self::InAGame(name) => write!(
                f,
                "{name} is in a game of chess already (and you may be): one game at a time each"
            ),
            Self::Yourself => f.write_str("you cannot challenge yourself"),
            Self::NotAvailable(name) => write!(f, "{name} is not at hand to play"),
            Self::NoSuchMember(name) => write!(f, "nobody on the team is called `{name}`"),
        }
    }
}

/// One game, as kept: who plays, and the moves so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// Its number.
    pub id: i64,
    /// The player with white.
    pub white: MemberId,
    /// The player with black.
    pub black: MemberId,
    /// The moves played, in standard notation, white's first.
    pub moves: Vec<String>,
    /// Where it stands.
    pub status: Status,
    /// How it ended, once it has.
    pub reason: String,
    /// When it began.
    pub started_at: Millis,
    /// When the last move was made (when it began, if none was).
    pub last_move_at: Millis,
}

/// The board as a player or the screen sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The position in Forsyth-Edwards notation.
    pub fen: String,
    /// The board drawn in plain text, white at the bottom.
    pub ascii: String,
    /// Eight rows of eight letters from the eighth rank down (capitals white, a dot for an
    /// empty square), for the screen to draw.
    pub rows: Vec<String>,
    /// Whether it is white to move.
    pub white_to_move: bool,
    /// Whether the player to move is in check.
    pub check: bool,
}

/// The result of a move that was played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The move as written down, with its check or mate sign.
    pub san: String,
    /// What ended the game, if this move did.
    pub ended: Option<(Status, String)>,
}

/// A game replayed: the position, and every position it has been in (the first and the last
/// included), for repetitions.
struct Replayed {
    board: Board,
    seen: Vec<Board>,
}

fn replay(moves: &[String]) -> Option<Replayed> {
    let mut board = Board::default();
    let mut seen = vec![board.clone()];
    for said in moves {
        let m = parse_san_move(&board, said).ok()?;
        board.play(m);
        seen.push(board.clone());
    }
    Some(Replayed { board, seen })
}

/// Whether neither side can ever give mate: only kings, or a king and one minor piece, or
/// bishops that all stand on squares of one colour.
fn insufficient_material(board: &Board) -> bool {
    let heavy = board.pieces(Piece::Pawn) | board.pieces(Piece::Rook) | board.pieces(Piece::Queen);
    if !heavy.is_empty() {
        return false;
    }
    let (knights, bishops) = (board.pieces(Piece::Knight), board.pieces(Piece::Bishop));
    if knights.len() + bishops.len() <= 1 {
        return true;
    }
    if !knights.is_empty() {
        return false;
    }
    let shade = |sq: Square| (sq.file() as usize + sq.rank() as usize) % 2;
    let mut shades = bishops.iter().map(shade);
    let first = shades.next();
    shades.all(|s| Some(s) == first)
}

/// How the game ended with this position, if it did: mate, stalemate, nobody able to mate,
/// the same position a third time, or fifty moves with no capture or pawn move. `seen` is
/// every position the game has been in, this one last.
fn ending(board: &Board, seen: &[Board]) -> Option<(Status, String)> {
    let no_moves = !board.generate_moves(|_| true);
    if no_moves && board.status() == GameStatus::Won {
        let winner = match board.side_to_move() {
            Color::White => Status::BlackWon,
            Color::Black => Status::WhiteWon,
        };
        return Some((winner, "checkmate".to_owned()));
    }
    if no_moves {
        return Some((Status::Draw, "stalemate".to_owned()));
    }
    if insufficient_material(board) {
        return Some((Status::Draw, "insufficient material to mate".to_owned()));
    }
    if seen.iter().filter(|b| b.same_position(board)).count() >= 3 {
        return Some((Status::Draw, "the same position a third time".to_owned()));
    }
    if u32::from(board.halfmove_clock()) >= FIFTY_MOVES {
        return Some((
            Status::Draw,
            "fifty moves without a capture or a pawn move".to_owned(),
        ));
    }
    None
}

/// The legal moves in notation, in order: for the tests, since no player is told them.
#[cfg(test)]
fn legal_moves(board: &Board) -> Vec<String> {
    let mut moves = Vec::new();
    board.generate_moves(|piece_moves| {
        moves.extend(piece_moves);
        false
    });
    let mut all: Vec<String> = moves
        .into_iter()
        .map(|m| display_san_move(board, m).to_string())
        .collect();
    all.sort();
    all
}

/// Whether a text has the shape of a move in standard notation (`e4`, `Nbd7`, `exd5`, `e8=Q+`,
/// `O-O`), whether or not it is legal: to tell "not a move" from "not a legal move".
fn looks_like_a_move(said: &str) -> bool {
    let said = said.trim_end_matches(['+', '#']);
    if said == "O-O" || said == "O-O-O" {
        return true;
    }
    let promoted = said
        .strip_suffix(['Q', 'R', 'B', 'N'])
        .map(|rest| rest.strip_suffix('=').unwrap_or(rest));
    let body = promoted.unwrap_or(said);
    let mut backwards = body.chars().rev();
    let (Some(rank), Some(file)) = (backwards.next(), backwards.next()) else {
        return false;
    };
    let rest: String = backwards.rev().collect();
    ('1'..='8').contains(&rank)
        && ('a'..='h').contains(&file)
        && rest.chars().count() <= 4
        && rest.chars().all(|c| "KQRBNabcdefgh12345678x".contains(c))
}

/// The letter for what stands on a square: capitals for white, small for black, a dot if empty.
fn letter(board: &Board, square: Square) -> char {
    let (Some(piece), Some(color)) = (board.piece_on(square), board.color_on(square)) else {
        return '.';
    };
    let said = char::from(piece);
    if color == Color::White {
        said.to_ascii_uppercase()
    } else {
        said
    }
}

impl Game {
    /// A new game, white to move.
    #[must_use]
    pub fn new(id: i64, white: MemberId, black: MemberId, at: Millis) -> Self {
        Self {
            id,
            white,
            black,
            moves: Vec::new(),
            status: Status::InProgress,
            reason: String::new(),
            started_at: at,
            last_move_at: at,
        }
    }

    /// Whether the game is still being played.
    #[must_use]
    pub fn live(&self) -> bool {
        self.status == Status::InProgress
    }

    /// Whether a member plays in it.
    #[must_use]
    pub fn plays(&self, member: MemberId) -> bool {
        self.white == member || self.black == member
    }

    /// The player whose move it is: white moves first, so on every even move.
    #[must_use]
    pub fn to_move(&self) -> MemberId {
        if self.moves.len().is_multiple_of(2) {
            self.white
        } else {
            self.black
        }
    }

    /// The other player, if `member` plays.
    #[must_use]
    pub fn opponent(&self, member: MemberId) -> Option<MemberId> {
        if member == self.white {
            return Some(self.black);
        }
        (member == self.black).then_some(self.white)
    }

    /// Who won, if somebody has.
    #[must_use]
    pub fn winner(&self) -> Option<MemberId> {
        match self.status {
            Status::WhiteWon => Some(self.white),
            Status::BlackWon => Some(self.black),
            Status::InProgress | Status::Draw => None,
        }
    }

    /// The board now: the position, drawn. `None` only if the kept moves
    /// do not replay, which a game built here never does.
    #[must_use]
    pub fn view(&self) -> Option<View> {
        let board = replay(&self.moves)?.board;
        let mut rows = Vec::new();
        let mut ascii = String::from("  a b c d e f g h\n");
        for rank in Rank::ALL.into_iter().rev() {
            let row: String = File::ALL
                .into_iter()
                .map(|file| letter(&board, Square::new(file, rank)))
                .collect();
            let spaced: Vec<String> = row.chars().map(String::from).collect();
            let number = rank as usize + 1;
            ascii.push_str(&format!("{number} {} {number}\n", spaced.join(" ")));
            rows.push(row);
        }
        ascii.push_str("  a b c d e f g h");
        Some(View {
            fen: board.to_string(),
            ascii,
            rows,
            white_to_move: board.side_to_move() == Color::White,
            check: !board.checkers().is_empty(),
        })
    }

    /// The moves in the way they are written down: `1. e4 e5 2. Nf3`. `last` limits it to
    /// that many of the latest moves (and says where it starts), `None` gives all of them.
    #[must_use]
    pub fn move_text(&self, last: Option<usize>) -> String {
        let from = last.map_or(0, |n| self.moves.len().saturating_sub(n));
        let mut text = String::new();
        for (i, m) in self.moves.iter().enumerate().skip(from) {
            if !text.is_empty() {
                text.push(' ');
            }
            if i % 2 == 0 {
                text.push_str(&format!("{}. ", i / 2 + 1));
            } else if text.is_empty() {
                text.push_str(&format!("{}... ", i / 2 + 1));
            }
            text.push_str(m);
        }
        text
    }

    /// Plays a move for `mover`, if the game is on, it is `mover`'s move, and the move is legal.
    /// The game is changed only when it is: the move kept, the time noted, and the game ended
    /// if the move ended it.
    ///
    /// # Errors
    /// What is wrong with the move: [`Refused::Over`], [`Refused::NotYourTurn`],
    /// [`Refused::NotAMove`] or [`Refused::Illegal`].
    pub fn play(&mut self, mover: MemberId, said: &str, now: Millis) -> Result<Moved, Refused> {
        if !self.live() {
            return Err(Refused::Over);
        }
        if !self.plays(mover) {
            return Err(Refused::NoGame);
        }
        if self.to_move() != mover {
            return Err(Refused::NotYourTurn);
        }
        let said = said.trim();
        let Some(mut played) = replay(&self.moves) else {
            return Err(Refused::Over);
        };
        let notation = said.replace('0', "O");
        if !looks_like_a_move(&notation) {
            return Err(Refused::NotAMove(said.to_owned()));
        }
        let Ok(m) = parse_san_move(&played.board, &notation) else {
            return Err(Refused::Illegal {
                said: said.to_owned(),
            });
        };
        let written = display_san_move(&played.board, m).to_string();
        played.board.play(m);
        played.seen.push(played.board.clone());
        self.moves.push(written.clone());
        self.last_move_at = now;
        let ended = ending(&played.board, &played.seen);
        if let Some((status, reason)) = &ended {
            self.status = *status;
            self.reason.clone_from(reason);
        }
        Ok(Moved {
            san: written,
            ended,
        })
    }

    /// Whether the game has waited a week for a move.
    #[must_use]
    pub fn stale(&self, now: Millis) -> bool {
        self.live() && now - self.last_move_at >= ADJOURN_AFTER_MS
    }

    /// Ends a live game as lost by the side to move, which was given its turns and did not
    /// move. Returns whether it did.
    pub fn forfeit(&mut self) -> bool {
        if !self.live() {
            return false;
        }
        let white_stopped = self.to_move() == self.white;
        self.status = if white_stopped {
            Status::BlackWon
        } else {
            Status::WhiteWon
        };
        let side = if white_stopped { "white" } else { "black" };
        self.reason = format!("{side} stopped moving, and forfeits");
        true
    }

    /// Adjourns the game as a draw, if it is stale. Returns whether it did.
    pub fn adjourn(&mut self, now: Millis) -> bool {
        if !self.stale(now) {
            return false;
        }
        self.status = Status::Draw;
        self.reason = "adjourned after a week without a move".to_owned();
        true
    }
}

/// How one member stands at chess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// Who.
    pub member: MemberId,
    /// Games won.
    pub wins: u32,
    /// Games lost.
    pub losses: u32,
    /// Games drawn, adjourned ones included.
    pub draws: u32,
}

/// The chess leaderboard, from the finished games: most wins first, then fewest losses, then
/// the member's number.
#[must_use]
pub fn standings(games: &[Game]) -> Vec<Standing> {
    let mut table: Vec<Standing> = Vec::new();
    for game in games.iter().filter(|g| !g.live()) {
        for member in [game.white, game.black] {
            let at = table.iter().position(|s| s.member == member);
            let at = at.unwrap_or_else(|| {
                table.push(Standing {
                    member,
                    wins: 0,
                    losses: 0,
                    draws: 0,
                });
                table.len() - 1
            });
            match game.winner() {
                Some(winner) if winner == member => table[at].wins += 1,
                Some(_) => table[at].losses += 1,
                None => table[at].draws += 1,
            }
        }
    }
    table.sort_by_key(|s| (std::cmp::Reverse(s.wins), s.losses, s.member));
    table
}

/// How a finished game reads: `Bo beat Dee by checkmate` or `Bo and Dee drew by stalemate`.
#[must_use]
pub fn result_said(game: &Game, white: &str, black: &str) -> String {
    match game.status {
        Status::InProgress => format!("{white} and {black} are playing"),
        Status::WhiteWon => format!("{white} beat {black} at chess by {}", game.reason),
        Status::BlackWon => format!("{black} beat {white} at chess by {}", game.reason),
        Status::Draw => format!("{white} and {black} drew at chess: {}", game.reason),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    const WHITE: MemberId = MemberId(1);
    const BLACK: MemberId = MemberId(2);

    fn game() -> Game {
        Game::new(1, WHITE, BLACK, 100)
    }

    /// Plays the moves, white first, each by whose turn it is; panics on a refusal.
    fn play_all(game: &mut Game, moves: &[&str]) {
        for (i, m) in moves.iter().enumerate() {
            let mover = if i % 2 == 0 { WHITE } else { BLACK };
            game.play(mover, m, 200 + i as i64)
                .unwrap_or_else(|e| panic!("move {} `{m}`: {e}", i + 1));
        }
    }

    fn position(fen: &str) -> Board {
        Board::from_str(fen).unwrap()
    }

    #[test]
    fn a_pawn_on_the_last_rank_promotes_however_the_promotion_is_written() {
        let to_the_edge = ["a4", "b5", "axb5", "a6", "bxa6", "Bb7", "axb7", "Nc6"];
        for (said, kept, piece) in [
            ("bxa8=Q", "bxa8=Q", 'Q'),
            ("bxa8Q", "bxa8=Q", 'Q'),
            ("bxa8=Q+", "bxa8=Q", 'Q'),
            ("bxa8=N", "bxa8=N", 'N'),
        ] {
            let mut g = game();
            play_all(&mut g, &to_the_edge);
            let moved = g.play(WHITE, said, 300).unwrap();
            assert_eq!(moved.san, kept, "{said}");
            assert_eq!(g.moves.last().map(String::as_str), Some(kept));
            let board = replay(&g.moves).unwrap().board;
            assert!(
                board.to_string().starts_with(&format!("{piece}2qkbnr/")),
                "{said}: {board}"
            );
        }
        // A pawn move to the last rank with no piece named is a promotion's shape, but the
        // rules need the piece: it is refused, and nothing else is said.
        let mut g = game();
        play_all(&mut g, &to_the_edge);
        assert_eq!(
            g.play(WHITE, "bxa8", 300),
            Err(Refused::Illegal {
                said: "bxa8".to_owned()
            })
        );
    }

    #[test]
    fn the_board_at_the_start_has_twenty_legal_moves_and_white_to_move() {
        let view = game().view().unwrap();
        assert_eq!(
            view.fen,
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        );
        let legal = legal_moves(&position(&view.fen));
        assert_eq!(legal.len(), 20);
        assert!(legal.contains(&"e4".to_owned()) && legal.contains(&"Nf3".to_owned()));
        assert!(view.white_to_move && !view.check);
        assert_eq!(view.rows[0], "rnbqkbnr");
        assert_eq!(view.rows[7], "RNBQKBNR");
        assert!(view.ascii.contains("8 r n b q k b n r 8"), "{}", view.ascii);
        assert!(view.ascii.contains("1 R N B Q K B N R 1"), "{}", view.ascii);
        assert_eq!(game().to_move(), WHITE);
    }

    #[test]
    fn moves_alternate_and_the_wrong_player_is_refused() {
        let mut g = game();
        assert_eq!(g.play(BLACK, "e5", 1), Err(Refused::NotYourTurn));
        let moved = g.play(WHITE, "e4", 2).unwrap();
        assert_eq!(moved.san, "e4");
        assert_eq!(g.to_move(), BLACK);
        assert_eq!(g.last_move_at, 2);
        assert_eq!(g.play(WHITE, "d4", 3), Err(Refused::NotYourTurn));
        assert_eq!(g.moves, ["e4"], "a refused move changes nothing");
        assert_eq!(g.play(MemberId(9), "d4", 3), Err(Refused::NoGame));
        assert_eq!(g.opponent(WHITE), Some(BLACK));
        assert_eq!(g.opponent(MemberId(9)), None);
    }

    #[test]
    fn an_illegal_move_is_refused_saying_only_that_and_changes_nothing() {
        let mut g = game();
        let refused = g.play(WHITE, "e5", 1);
        assert_eq!(
            refused,
            Err(Refused::Illegal {
                said: "e5".to_owned()
            })
        );
        // Standard play: the player is told the move is not legal, never which moves are.
        assert_eq!(refused.unwrap_err().to_string(), "`e5` is not legal here");
        assert!(g.moves.is_empty());
        assert_eq!(g.last_move_at, 100);
        assert!(matches!(
            g.play(WHITE, "banana", 1),
            Err(Refused::NotAMove(_))
        ));
        assert!(matches!(g.play(WHITE, "", 1), Err(Refused::NotAMove(_))));
        // Moving through your own piece, and into check, are not legal either.
        assert!(matches!(
            g.play(WHITE, "Bc4", 1),
            Err(Refused::Illegal { .. })
        ));
        let mut g = game();
        play_all(&mut g, &["f3", "e5", "Kf2", "Qh4"]);
        // The king on f2 is in check from the queen on h4: g3 blocks it, Kg3 steps into it, and
        // Ke1 stays on the queen's line.
        assert!(g.view().unwrap().check);
        assert!(matches!(
            g.play(WHITE, "Kg3", 9),
            Err(Refused::Illegal { .. })
        ));
        assert!(matches!(
            g.play(WHITE, "Ke1", 9),
            Err(Refused::Illegal { .. })
        ));
        assert!(g.play(WHITE, "g3", 9).is_ok());
    }

    #[test]
    fn a_move_is_written_down_with_its_check_and_castling_in_standard_notation() {
        let mut g = game();
        play_all(&mut g, &["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O"]);
        assert_eq!(g.moves.last().unwrap(), "O-O");
        // A check sign is added if the player left it off, and tolerated if they wrote it.
        let mut g = game();
        play_all(&mut g, &["e4", "f5", "Qh5"]);
        assert_eq!(g.moves.last().unwrap(), "Qh5+");
        let mut g = game();
        play_all(&mut g, &["e4", "f5", "Qh5+"]);
        assert_eq!(g.moves.last().unwrap(), "Qh5+");
        assert_eq!(g.move_text(None), "1. e4 f5 2. Qh5+");
        assert_eq!(g.to_move(), BLACK);
    }

    #[test]
    fn castling_may_be_written_with_zeros() {
        let mut g = game();
        play_all(&mut g, &["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "0-0"]);
        assert_eq!(g.moves.last().unwrap(), "O-O");
    }

    #[test]
    fn checkmate_ends_the_game_and_names_the_winner() {
        let mut g = game();
        play_all(&mut g, &["e4", "e5", "Bc4", "Nc6", "Qh5", "Nf6"]);
        let moved = g.play(WHITE, "Qxf7", 9).unwrap();
        assert_eq!(moved.san, "Qxf7#");
        assert_eq!(
            moved.ended,
            Some((Status::WhiteWon, "checkmate".to_owned()))
        );
        assert_eq!((g.status, g.winner()), (Status::WhiteWon, Some(WHITE)));
        assert_eq!(g.reason, "checkmate");
        assert!(!g.live());
        assert_eq!(g.play(BLACK, "Ke7", 10), Err(Refused::Over));
        assert_eq!(
            result_said(&g, "Bo", "Dee"),
            "Bo beat Dee at chess by checkmate"
        );
    }

    #[test]
    fn black_can_win_too() {
        let mut g = game();
        play_all(&mut g, &["f3", "e5", "g4", "Qh4"]);
        assert_eq!(g.status, Status::BlackWon, "the fool's mate");
        assert_eq!(g.winner(), Some(BLACK));
        assert_eq!(
            result_said(&g, "Bo", "Dee"),
            "Dee beat Bo at chess by checkmate"
        );
    }

    #[test]
    fn stalemate_is_a_draw() {
        let mut g = game();
        play_all(
            &mut g,
            &[
                "e3", "a5", "Qh5", "Ra6", "Qxa5", "h5", "h4", "Rah6", "Qxc7", "f6", "Qxd7+", "Kf7",
                "Qxb7", "Qd3", "Qxb8", "Qh7", "Qxc8", "Kg6", "Qe6",
            ],
        );
        assert_eq!(g.status, Status::Draw);
        assert_eq!(g.reason, "stalemate");
        assert_eq!(g.winner(), None);
        assert_eq!(
            result_said(&g, "Bo", "Dee"),
            "Bo and Dee drew at chess: stalemate"
        );
    }

    #[test]
    fn insufficient_material_and_the_fifty_move_rule_end_a_game() {
        let bare = position("8/8/4k3/8/8/4K3/8/8 w - - 0 1");
        let dead = Some((Status::Draw, "insufficient material to mate".to_owned()));
        assert_eq!(ending(&bare, &[bare.clone()]), dead);
        let rook = position("8/8/4k3/8/8/4K3/8/7R w - - 99 80");
        assert_eq!(ending(&rook, &[rook.clone()]), None);
        let tired = position("8/8/4k3/8/8/4K3/8/7R w - - 100 80");
        assert_eq!(
            ending(&tired, &[tired.clone()]),
            Some((
                Status::Draw,
                "fifty moves without a capture or a pawn move".to_owned()
            ))
        );
        // A pawn can always promote.
        let pawn = position("8/4P3/4k3/8/8/4K3/8/8 w - - 0 1");
        assert_eq!(ending(&pawn, &[pawn.clone()]), None);
        // A lone minor piece cannot mate; bishops on squares of one colour cannot either; but
        // bishops on both colours, or two knights, might be mated against a blunder.
        for (fen, over) in [
            ("8/8/4k3/8/8/4K3/8/6B1 w - - 0 1", true),
            ("8/8/4k3/8/8/4K3/8/6N1 w - - 0 1", true),
            ("8/8/4kb2/8/8/4K3/8/6B1 w - - 0 1", true),
            ("8/8/4kb2/8/8/4K3/8/5B2 w - - 0 1", false),
            ("8/8/4k3/8/8/4K3/8/5BB1 w - - 0 1", false),
            ("8/8/4k3/8/8/4K3/8/5NN1 w - - 0 1", false),
        ] {
            let board = position(fen);
            assert_eq!(ending(&board, &[board.clone()]).is_some(), over, "{fen}");
        }
    }

    #[test]
    fn the_same_position_a_third_time_is_a_draw() {
        let mut g = game();
        play_all(&mut g, &["Nf3", "Nf6", "Ng1", "Ng8", "Nf3", "Nf6", "Ng1"]);
        assert!(g.live(), "twice is not enough");
        let moved = g.play(BLACK, "Ng8", 99).unwrap();
        assert_eq!(
            moved.ended,
            Some((Status::Draw, "the same position a third time".to_owned()))
        );
        assert_eq!(g.status, Status::Draw);
    }

    #[test]
    fn a_game_idle_for_a_week_is_adjourned_as_a_draw() {
        let mut g = game();
        play_all(&mut g, &["e4"]);
        let at = g.last_move_at;
        assert!(!g.stale(at + ADJOURN_AFTER_MS - 1));
        assert!(!g.adjourn(at + ADJOURN_AFTER_MS - 1));
        assert!(g.live());
        assert!(g.adjourn(at + ADJOURN_AFTER_MS));
        assert_eq!(g.status, Status::Draw);
        assert!(g.reason.contains("adjourned"));
        assert!(!g.adjourn(at + 2 * ADJOURN_AFTER_MS), "once");
    }

    #[test]
    fn the_moves_read_as_a_score_and_a_tail_of_them_starts_where_it_should() {
        let mut g = game();
        play_all(&mut g, &["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        assert_eq!(g.move_text(None), "1. e4 e5 2. Nf3 Nc6 3. Bb5");
        assert_eq!(g.move_text(Some(2)), "2... Nc6 3. Bb5");
        assert_eq!(g.move_text(Some(3)), "2. Nf3 Nc6 3. Bb5");
        assert_eq!(game().move_text(None), "");
    }

    #[test]
    fn the_leaderboard_counts_finished_games_only() {
        let done = |id, white, black, status| Game {
            status,
            reason: "checkmate".to_owned(),
            ..Game::new(id, MemberId(white), MemberId(black), 0)
        };
        let games = [
            done(1, 1, 2, Status::WhiteWon),
            done(2, 2, 1, Status::Draw),
            done(3, 3, 1, Status::BlackWon),
            Game::new(4, MemberId(2), MemberId(3), 0),
        ];
        let table = standings(&games);
        let row = |m: i64| {
            table
                .iter()
                .find(|s| s.member == MemberId(m))
                .unwrap()
                .clone()
        };
        assert_eq!(table[0].member, MemberId(1));
        assert_eq!((row(1).wins, row(1).losses, row(1).draws), (2, 0, 1));
        assert_eq!((row(2).wins, row(2).losses, row(2).draws), (0, 1, 1));
        assert_eq!((row(3).wins, row(3).losses, row(3).draws), (0, 1, 0));
    }

    #[test]
    fn a_status_reads_back_from_its_name() {
        for s in [
            Status::InProgress,
            Status::WhiteWon,
            Status::BlackWon,
            Status::Draw,
        ] {
            assert_eq!(Status::from_name(s.name()), Some(s));
        }
        assert_eq!(Status::from_name("abandoned"), None);
    }
}
