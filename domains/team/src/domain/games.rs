//! Ping-pong and pool in the office (ADR 0121). Two members who are free play a match that
//! costs nothing: no model is asked, and HQ's engine plays it in code, point by point or shot
//! by shot, from a small seeded generator, so the same seed always gives the same match. Each
//! member has a hidden skill at each game, set from its number and sharpened a little by
//! practice. Every match is kept as a [`Record`], and the leaderboards are read from the
//! records alone.

use super::work::{MemberId, Millis};

/// A game HQ plays for the team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Table tennis: first to eleven, and two clear.
    PingPong,
    /// Eight-ball, simplified: pot your seven, then the eight.
    Pool,
}

impl Kind {
    /// Every game, in the order the office lists them.
    pub const ALL: [Self; 2] = [Self::PingPong, Self::Pool];

    /// The name as stored.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PingPong => "ping_pong",
            Self::Pool => "pool",
        }
    }

    /// The name as people say it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PingPong => "ping-pong",
            Self::Pool => "pool",
        }
    }

    /// The kind with this stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// Points to win a game of ping-pong, when two clear.
pub const PING_PONG_TO: u32 = 11;
/// Balls a player pots before the eight: the seven of their group.
pub const GROUP: u32 = 7;
/// The lowest a member's starting skill is, in percent.
pub const SKILL_LOW: u32 = 35;
/// How many different starting skills there are above the lowest.
pub const SKILL_SPAN: u32 = 31;
/// Games of one kind that give a member a point of skill.
pub const PRACTICE_EVERY: usize = 10;
/// The most points of skill practice gives.
pub const PRACTICE_MOST: u32 = 10;
/// The least time between two matches of one game.
pub const GAP_MS: Millis = 20 * 60 * 1000;
/// How long a member who has just played rests before it is free for any match.
pub const REST_MS: Millis = 10 * 60 * 1000;
/// The chance, in percent, that a missed shot at the eight is a scratch, which loses.
const SCRATCH_ON_EIGHT: u32 = 20;
/// A safety stop: a match is never longer than this many rallies or turns.
const MOST_STEPS: u32 = 2_000;

/// A small deterministic generator (splitmix64): no dependency, and the same seed always
/// gives the same numbers.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator from a seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (and 0 when `n` is 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }

    /// True with this chance in a hundred.
    pub fn percent(&mut self, chance: u32) -> bool {
        self.below(100) < u64::from(chance)
    }
}

/// The seed of a match: the engine's clock, who plays, and how many matches there have been,
/// mixed so that a minute apart or a member apart gives a different match.
#[must_use]
pub fn seed(at: Millis, a: MemberId, b: MemberId, played: usize) -> u64 {
    let mut mix = Rng::new(at.unsigned_abs());
    let _ = mix.next_u64();
    let mut rng = Rng::new(mix.next_u64() ^ a.0.unsigned_abs().wrapping_mul(0x1000_0000_01B3));
    let _ = rng.next_u64();
    Rng::new(rng.next_u64() ^ b.0.unsigned_abs().wrapping_mul(0x0100_0000_01B3) ^ played as u64)
        .next_u64()
}

/// A member's skill at a game, in percent: a starting skill from its number, hidden, and a
/// point more for every ten games of that kind it has played, up to ten points.
#[must_use]
pub fn skill(member: MemberId, kind: Kind, played: usize) -> u32 {
    let tag = match kind {
        Kind::PingPong => 0x51,
        Kind::Pool => 0xA3,
    };
    let mut rng = Rng::new(member.0.unsigned_abs().wrapping_mul(0x9E37) ^ tag);
    let _ = rng.next_u64();
    let start = SKILL_LOW + u32::try_from(rng.below(u64::from(SKILL_SPAN))).unwrap_or(0);
    let practice = u32::try_from(played / PRACTICE_EVERY)
        .unwrap_or(PRACTICE_MOST)
        .min(PRACTICE_MOST);
    start + practice
}

/// How a match went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played {
    /// The first player's score.
    pub a: u32,
    /// The second player's score.
    pub b: u32,
    /// Whether the first player won.
    pub a_won: bool,
    /// A word on how it ended, if it ended oddly: empty otherwise.
    pub note: String,
}

/// The chance, in percent, that the first of two players wins a rally.
fn rally_chance(a: u32, b: u32) -> u32 {
    (50 + (i64::from(a) - i64::from(b)) / 2).clamp(10, 90) as u32
}

/// A match of ping-pong between players of these skills: first to eleven, and two clear.
pub fn play_ping_pong(skill_a: u32, skill_b: u32, rng: &mut Rng) -> Played {
    let chance = rally_chance(skill_a, skill_b);
    let (mut a, mut b) = (0u32, 0u32);
    for _ in 0..MOST_STEPS {
        if rng.percent(chance) {
            a += 1;
        } else {
            b += 1;
        }
        let top = a.max(b);
        if top >= PING_PONG_TO && a.abs_diff(b) >= 2 {
            break;
        }
    }
    Played {
        a,
        b,
        a_won: a > b,
        note: String::new(),
    }
}

/// A game of pool, eight-ball simplified: the players take turns at the table, each shot
/// potting by skill, and a player who pots goes on. Each pots their seven and then the eight;
/// whoever pots the eight first wins. A missed shot at the eight may be a scratch, which
/// loses. The scores are the balls each potted, the eight counting as the eighth.
/// `names` are how the two are called in a note about a scratch.
pub fn play_pool(skill_a: u32, skill_b: u32, names: (&str, &str), rng: &mut Rng) -> Played {
    let pot = |skill: u32| skill.clamp(25, 85);
    let skills = [pot(skill_a), pot(skill_b)];
    let mut potted = [0u32, 0u32];
    let mut at = 0usize;
    let (mut winner, mut note) = (None, String::new());
    'match_: for _ in 0..MOST_STEPS {
        // One visit to the table: shots go on while they drop.
        loop {
            if rng.percent(skills[at]) {
                potted[at] += 1;
                if potted[at] > GROUP {
                    winner = Some(at);
                    break 'match_;
                }
                continue;
            }
            if potted[at] == GROUP && rng.percent(SCRATCH_ON_EIGHT) {
                winner = Some(1 - at);
                let who = if at == 0 { names.0 } else { names.1 };
                note = format!("{who} scratched on the 8");
                break 'match_;
            }
            break;
        }
        at = 1 - at;
    }
    // A match that never ended goes to whoever had potted more, the first player on a tie.
    let a_won = winner.map_or(potted[0] >= potted[1], |w| w == 0);
    Played {
        a: potted[0],
        b: potted[1],
        a_won,
        note,
    }
}

/// One match, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Its number.
    pub id: i64,
    /// The game.
    pub kind: Kind,
    /// The first player.
    pub a: MemberId,
    /// The second player.
    pub b: MemberId,
    /// The first player's score.
    pub score_a: u32,
    /// The second player's score.
    pub score_b: u32,
    /// Who won.
    pub winner: MemberId,
    /// How it ended, if oddly.
    pub note: String,
    /// When it was played.
    pub at: Millis,
}

impl Record {
    /// The player who lost.
    #[must_use]
    pub fn loser(&self) -> MemberId {
        if self.winner == self.a {
            self.b
        } else {
            self.a
        }
    }

    /// The winner's score and then the loser's.
    #[must_use]
    pub fn scores(&self) -> (u32, u32) {
        if self.winner == self.a {
            (self.score_a, self.score_b)
        } else {
            (self.score_b, self.score_a)
        }
    }

    /// The match in words for the feed: `Bo beat Dee 11–8 at ping-pong`.
    #[must_use]
    pub fn said(&self, winner: &str, loser: &str) -> String {
        let (won, lost) = self.scores();
        let mut text = format!(
            "{winner} beat {loser} {won}\u{2013}{lost} at {}",
            self.kind.label()
        );
        if !self.note.is_empty() {
            text.push_str(&format!(" ({})", self.note));
        }
        text
    }
}

/// How one member stands at a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// Who.
    pub member: MemberId,
    /// Matches won.
    pub wins: u32,
    /// Matches lost.
    pub losses: u32,
    /// The run it is on: positive for wins in a row, negative for losses in a row.
    pub streak: i32,
}

/// The leaderboard for a game, from the records: most wins first, then fewest losses, then
/// the member's number.
#[must_use]
pub fn standings(kind: Kind, records: &[Record]) -> Vec<Standing> {
    fn entry(table: &mut Vec<Standing>, member: MemberId) -> &mut Standing {
        let at = table.iter().position(|s| s.member == member);
        let at = at.unwrap_or_else(|| {
            table.push(Standing {
                member,
                wins: 0,
                losses: 0,
                streak: 0,
            });
            table.len() - 1
        });
        &mut table[at]
    }
    let mut table: Vec<Standing> = Vec::new();
    for record in records.iter().filter(|r| r.kind == kind) {
        let won = entry(&mut table, record.winner);
        won.wins += 1;
        won.streak = if won.streak > 0 { won.streak + 1 } else { 1 };
        let lost = entry(&mut table, record.loser());
        lost.losses += 1;
        lost.streak = if lost.streak < 0 { lost.streak - 1 } else { -1 };
    }
    table.sort_by_key(|s| (std::cmp::Reverse(s.wins), s.losses, s.member));
    table
}

/// How many games of a kind a member has played.
#[must_use]
pub fn played_by(member: MemberId, kind: Kind, records: &[Record]) -> usize {
    records
        .iter()
        .filter(|r| r.kind == kind && (r.a == member || r.b == member))
        .count()
}

/// Whether a match of this kind is due: none within the last twenty minutes, so a game is
/// played at most three times an hour, and one at a time.
#[must_use]
pub fn due(kind: Kind, records: &[Record], now: Millis) -> bool {
    records
        .iter()
        .filter(|r| r.kind == kind)
        .all(|r| now - r.at >= GAP_MS)
}

/// Whether a member has played anything lately: it takes a rest before the next match.
#[must_use]
pub fn rested(member: MemberId, records: &[Record], now: Millis) -> bool {
    records
        .iter()
        .filter(|r| r.a == member || r.b == member)
        .all(|r| now - r.at >= REST_MS)
}

/// Two of the free members, picked by the generator: the first and the second player.
#[must_use]
pub fn pick_two(free: &[MemberId], rng: &mut Rng) -> Option<(MemberId, MemberId)> {
    if free.len() < 2 {
        return None;
    }
    let first = usize::try_from(rng.below(free.len() as u64)).unwrap_or(0);
    let mut second = usize::try_from(rng.below(free.len() as u64 - 1)).unwrap_or(0);
    if second >= first {
        second += 1;
    }
    Some((free[first], free[second]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, kind: Kind, a: i64, b: i64, winner: i64, at: Millis) -> Record {
        Record {
            id,
            kind,
            a: MemberId(a),
            b: MemberId(b),
            score_a: 11,
            score_b: 7,
            winner: MemberId(winner),
            note: String::new(),
            at,
        }
    }

    #[test]
    fn the_same_seed_plays_the_same_match_and_another_seed_another() {
        let one = play_ping_pong(50, 50, &mut Rng::new(7));
        let again = play_ping_pong(50, 50, &mut Rng::new(7));
        assert_eq!(one, again);
        let differ = (0..20u64).any(|s| play_ping_pong(50, 50, &mut Rng::new(s)) != one);
        assert!(differ, "twenty seeds all played one match");
        let pool = play_pool(50, 50, ("A", "B"), &mut Rng::new(7));
        assert_eq!(pool, play_pool(50, 50, ("A", "B"), &mut Rng::new(7)));
    }

    #[test]
    fn a_fixed_seed_gives_a_fixed_ping_pong_score() {
        // Pinned: a change to the generator or the rules changes every kept match's story.
        let m = play_ping_pong(50, 50, &mut Rng::new(1));
        assert_eq!((m.a, m.b, m.a_won), (11, 7, true), "{m:?}");
        let m = play_ping_pong(40, 60, &mut Rng::new(2));
        assert_eq!((m.a, m.b, m.a_won), (11, 7, true), "{m:?}");
    }

    #[test]
    fn ping_pong_is_first_to_eleven_and_two_clear() {
        for seed in 0..500u64 {
            for (a, b) in [(35, 75), (50, 50), (75, 35)] {
                let m = play_ping_pong(a, b, &mut Rng::new(seed));
                let (top, low) = (m.a.max(m.b), m.a.min(m.b));
                assert!(top >= 11, "{m:?}");
                assert!(top - low >= 2, "{m:?}");
                // Nobody is more than two ahead past ten all: a long game ends on exactly two.
                if low >= 10 {
                    assert_eq!(top - low, 2, "{m:?}");
                }
                assert_eq!(m.a_won, m.a > m.b);
                assert!(m.note.is_empty());
            }
        }
    }

    #[test]
    fn a_better_player_wins_more_often() {
        let wins = (0..400u64)
            .filter(|s| play_ping_pong(70, 40, &mut Rng::new(*s)).a_won)
            .count();
        assert!(wins > 280, "the better player won only {wins} of 400");
        let pool = (0..400u64)
            .filter(|s| play_pool(80, 40, ("A", "B"), &mut Rng::new(*s)).a_won)
            .count();
        assert!(pool > 280, "the better potter won only {pool} of 400");
    }

    #[test]
    fn pool_is_won_by_clearing_the_table_or_lost_by_scratching_on_the_eight() {
        let (mut cleared, mut scratched) = (0, 0);
        for seed in 0..600u64 {
            let m = play_pool(55, 55, ("Bo", "Dee"), &mut Rng::new(seed));
            let (win, lose) = if m.a_won { (m.a, m.b) } else { (m.b, m.a) };
            if m.note.is_empty() {
                cleared += 1;
                assert_eq!(win, 8, "a clear win pots all eight: {m:?}");
                assert!(lose <= 7, "{m:?}");
                continue;
            }
            scratched += 1;
            // The loser had potted their seven and scratched on the eight.
            assert_eq!(lose, 7, "{m:?}");
            assert!(win <= 7, "{m:?}");
            let loser = if m.a_won { "Dee" } else { "Bo" };
            assert_eq!(m.note, format!("{loser} scratched on the 8"));
        }
        assert!(
            cleared > 0 && scratched > 0,
            "{cleared} cleared, {scratched} scratched"
        );
    }

    #[test]
    fn skill_is_hidden_per_member_and_game_and_grows_a_point_per_ten_games_to_a_cap() {
        let base = skill(MemberId(3), Kind::PingPong, 0);
        assert!(
            (SKILL_LOW..SKILL_LOW + SKILL_SPAN).contains(&base),
            "{base}"
        );
        assert_eq!(
            base,
            skill(MemberId(3), Kind::PingPong, 9),
            "nine games teach nothing"
        );
        assert_eq!(skill(MemberId(3), Kind::PingPong, 10), base + 1);
        assert_eq!(skill(MemberId(3), Kind::PingPong, 55), base + 5);
        assert_eq!(
            skill(MemberId(3), Kind::PingPong, 100_000),
            base + PRACTICE_MOST
        );
        // Members differ, and so do the games.
        let all: std::collections::HashSet<u32> = (1..40)
            .map(|m| skill(MemberId(m), Kind::PingPong, 0))
            .collect();
        assert!(
            all.len() > 8,
            "forty members, {} different skills",
            all.len()
        );
        assert!((1..40).any(|m| {
            skill(MemberId(m), Kind::PingPong, 0) != skill(MemberId(m), Kind::Pool, 0)
        }));
    }

    #[test]
    fn the_seed_depends_on_the_time_the_players_and_the_count() {
        let base = seed(1_000, MemberId(1), MemberId(2), 0);
        assert_eq!(base, seed(1_000, MemberId(1), MemberId(2), 0));
        assert_ne!(base, seed(1_001, MemberId(1), MemberId(2), 0));
        assert_ne!(base, seed(1_000, MemberId(2), MemberId(1), 0));
        assert_ne!(base, seed(1_000, MemberId(1), MemberId(3), 0));
        assert_ne!(base, seed(1_000, MemberId(1), MemberId(2), 1));
    }

    #[test]
    fn a_record_is_said_winner_first_and_the_leaderboard_counts_wins_losses_and_streaks() {
        let records = [
            record(1, Kind::PingPong, 1, 2, 1, 0),
            record(2, Kind::PingPong, 2, 1, 1, 0),
            record(3, Kind::PingPong, 2, 3, 3, 0),
            record(4, Kind::Pool, 2, 3, 2, 0),
        ];
        assert_eq!(
            records[0].said("Bo", "Dee"),
            "Bo beat Dee 11\u{2013}7 at ping-pong"
        );
        let swapped = Record {
            score_a: 4,
            score_b: 11,
            winner: MemberId(2),
            ..records[0].clone()
        };
        assert_eq!(swapped.scores(), (11, 4));
        assert_eq!(swapped.loser(), MemberId(1));
        let table = standings(Kind::PingPong, &records);
        let row = |m: i64| {
            table
                .iter()
                .find(|s| s.member == MemberId(m))
                .unwrap()
                .clone()
        };
        assert_eq!(table[0].member, MemberId(1), "two wins lead");
        assert_eq!((row(1).wins, row(1).losses, row(1).streak), (2, 0, 2));
        assert_eq!((row(2).wins, row(2).losses, row(2).streak), (0, 3, -3));
        assert_eq!((row(3).wins, row(3).losses, row(3).streak), (1, 0, 1));
        // The pool match is not on the ping-pong board.
        assert_eq!(standings(Kind::Pool, &records).len(), 2);
        assert_eq!(played_by(MemberId(2), Kind::PingPong, &records), 3);
        assert_eq!(played_by(MemberId(2), Kind::Pool, &records), 1);
    }

    #[test]
    fn a_streak_turns_over_when_the_result_does() {
        let records = [
            record(1, Kind::Pool, 1, 2, 1, 0),
            record(2, Kind::Pool, 1, 2, 1, 0),
            record(3, Kind::Pool, 1, 2, 2, 0),
        ];
        let table = standings(Kind::Pool, &records);
        let one = table.iter().find(|s| s.member == MemberId(1)).unwrap();
        assert_eq!(one.streak, -1);
        let two = table.iter().find(|s| s.member == MemberId(2)).unwrap();
        assert_eq!(two.streak, 1);
    }

    #[test]
    fn matches_are_spaced_so_a_game_is_played_a_few_times_an_hour() {
        let records = [record(1, Kind::PingPong, 1, 2, 1, 100)];
        assert!(!due(Kind::PingPong, &records, 100 + GAP_MS - 1));
        assert!(due(Kind::PingPong, &records, 100 + GAP_MS));
        assert!(
            due(Kind::Pool, &records, 101),
            "another game is its own table"
        );
        assert!(due(Kind::PingPong, &[], 0));
        assert!(!rested(MemberId(1), &records, 100 + REST_MS - 1));
        assert!(rested(MemberId(1), &records, 100 + REST_MS));
        assert!(rested(MemberId(9), &records, 101));
    }

    #[test]
    fn two_different_members_are_picked_or_none_when_fewer_than_two_are_free() {
        let free = [MemberId(1), MemberId(2), MemberId(3)];
        for s in 0..200u64 {
            let (a, b) = pick_two(&free, &mut Rng::new(s)).unwrap();
            assert_ne!(a, b);
            assert!(free.contains(&a) && free.contains(&b));
        }
        assert!(pick_two(&free[..1], &mut Rng::new(1)).is_none());
        assert!(pick_two(&[], &mut Rng::new(1)).is_none());
    }

    #[test]
    fn every_game_has_a_name_that_reads_back() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(Kind::from_name("darts"), None);
    }
}
