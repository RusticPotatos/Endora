//! The model league (ADR 0135, amended 2026-10-25): the brains play chess against each other in
//! the Holodeck, each side pinned to one brain whoever sits at the board, so that a table of
//! ratings says which model plays best. Every pair plays at least once; the pair that has
//! played least goes next, colours swapped from its last game. The ratings are Elo, summed
//! from the results in the order the games were played: arithmetic over the record, never a
//! model's account.

use super::brain::Brain;

/// Where every brain's rating starts.
pub const START: i64 = 1200;
/// How far one game moves a rating at most.
const K: f64 = 32.0;

/// One game of the league, as the record shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Played {
    /// The brain that played white.
    pub white: Brain,
    /// The brain that played black.
    pub black: Brain,
    /// How it ended, from white's side: 100 a win, 50 a draw, 0 a loss; `None` while live.
    pub score: Option<u32>,
}

/// One brain's line in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The brain.
    pub brain: Brain,
    /// Its rating.
    pub rating: i64,
    /// Games won.
    pub won: u32,
    /// Games drawn.
    pub drawn: u32,
    /// Games lost.
    pub lost: u32,
}

/// The next game: the pair of different brains that has played least (finished or not), the
/// earlier pair in `brains` on a tie; white is the one of the two that has had white less
/// often against the other, the earlier brain on a tie. `None` with fewer than two brains.
#[must_use]
pub fn next(brains: &[Brain], games: &[Played]) -> Option<(Brain, Brain)> {
    let mut best: Option<(usize, Brain, Brain)> = None;
    for (i, a) in brains.iter().enumerate() {
        for b in &brains[i + 1..] {
            let between = games
                .iter()
                .filter(|g| (g.white, g.black) == (*a, *b) || (g.white, g.black) == (*b, *a))
                .count();
            if best.is_none_or(|(least, _, _)| between < least) {
                best = Some((between, *a, *b));
            }
        }
    }
    let (_, a, b) = best?;
    let whites = |w: Brain, x: Brain| {
        games
            .iter()
            .filter(|g| (g.white, g.black) == (w, x))
            .count()
    };
    if whites(b, a) < whites(a, b) {
        return Some((b, a));
    }
    Some((a, b))
}

/// How many pairs of `brains` have not finished a game yet.
#[must_use]
pub fn pairs_left(brains: &[Brain], games: &[Played]) -> usize {
    let mut left = 0;
    for (i, a) in brains.iter().enumerate() {
        for b in &brains[i + 1..] {
            let met = games.iter().any(|g| {
                g.score.is_some()
                    && ((g.white, g.black) == (*a, *b) || (g.white, g.black) == (*b, *a))
            });
            if !met {
                left += 1;
            }
        }
    }
    left
}

/// The table: every brain in `brains`, rated from the finished games in the order played, best
/// first (the earlier brain in `brains` on a tie).
#[must_use]
pub fn table(brains: &[Brain], games: &[Played]) -> Vec<Row> {
    let mut rating: Vec<f64> = brains.iter().map(|_| START as f64).collect();
    let mut rows: Vec<Row> = brains
        .iter()
        .map(|b| Row {
            brain: *b,
            rating: START,
            won: 0,
            drawn: 0,
            lost: 0,
        })
        .collect();
    let at = |b: Brain| brains.iter().position(|x| *x == b);
    for game in games {
        let (Some(score), Some(w), Some(b)) = (game.score, at(game.white), at(game.black)) else {
            continue;
        };
        let expected = 1.0 / (1.0 + 10f64.powf((rating[b] - rating[w]) / 400.0));
        let got = f64::from(score) / 100.0;
        rating[w] += K * (got - expected);
        rating[b] -= K * (got - expected);
        match score {
            100 => {
                rows[w].won += 1;
                rows[b].lost += 1;
            }
            0 => {
                rows[w].lost += 1;
                rows[b].won += 1;
            }
            _ => {
                rows[w].drawn += 1;
                rows[b].drawn += 1;
            }
        }
    }
    for (row, r) in rows.iter_mut().zip(&rating) {
        #[allow(clippy::cast_possible_truncation)]
        let rounded = r.round() as i64;
        row.rating = rounded;
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.rating));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Brain; 3] = [Brain::Local, Brain::Mac, Brain::Haiku];

    fn game(white: Brain, black: Brain, score: Option<u32>) -> Played {
        Played {
            white,
            black,
            score,
        }
    }

    #[test]
    fn every_pair_plays_once_before_any_pair_plays_twice() {
        let mut games = Vec::new();
        for _ in 0..3 {
            let (w, b) = next(&ALL, &games).unwrap();
            games.push(game(w, b, Some(50)));
        }
        let mut pairs: Vec<_> = games
            .iter()
            .map(|g| {
                let mut p = [g.white.name(), g.black.name()];
                p.sort_unstable();
                p
            })
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        assert_eq!(pairs.len(), 3, "{games:?}");
        assert_eq!(pairs_left(&ALL, &games), 0);
    }

    #[test]
    fn a_rematch_swaps_the_colours() {
        let games = vec![game(Brain::Local, Brain::Mac, Some(100))];
        assert_eq!(
            next(&[Brain::Local, Brain::Mac], &games),
            Some((Brain::Mac, Brain::Local))
        );
    }

    #[test]
    fn a_game_still_being_played_counts_for_the_pairing_but_not_the_table() {
        let games = vec![game(Brain::Local, Brain::Mac, None)];
        assert_eq!(next(&ALL, &games), Some((Brain::Local, Brain::Haiku)));
        assert_eq!(pairs_left(&ALL, &games), 3);
        assert!(table(&ALL, &games).iter().all(|r| r.rating == START));
    }

    #[test]
    fn a_win_moves_both_ratings_by_the_same_amount_and_ranks_the_winner_first() {
        let games = vec![game(Brain::Mac, Brain::Haiku, Some(0))];
        let rows = table(&ALL, &games);
        assert_eq!(rows[0].brain, Brain::Haiku);
        assert_eq!(rows[0].rating, START + 16);
        assert_eq!(rows[0].won, 1);
        let mac = rows.iter().find(|r| r.brain == Brain::Mac).unwrap();
        assert_eq!((mac.rating, mac.lost), (START - 16, 1));
        let draw = table(&ALL, &[game(Brain::Local, Brain::Mac, Some(50))]);
        assert!(draw.iter().all(|r| r.rating == START));
        assert_eq!(draw[0].drawn + draw[1].drawn, 2);
    }

    #[test]
    fn fewer_than_two_brains_play_no_game() {
        assert_eq!(next(&[Brain::Local], &[]), None);
        assert_eq!(next(&[], &[]), None);
    }
}
