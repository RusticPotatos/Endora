//! A small seeded generator, and the fair pick of two from those free (ADR 0121): the league
//! seats its pairs with them, so the same seed always picks the same pair. Ping-pong and pool are
//! the browser's animation alone since ADR 0142, and keep no score.

use super::work::{MemberId, Millis};

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
}
