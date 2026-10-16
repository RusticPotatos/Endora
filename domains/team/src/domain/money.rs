//! Money: the team pool, each member's share of it per day, and what one turn may spend.
//!
//! Amounts are millionths of a dollar, so a model's per-token price adds up without rounding.

use super::work::Millis;

/// Millionths of a dollar.
pub type Micros = i64;

/// One dollar.
pub const DOLLAR: Micros = 1_000_000;
/// The team pool for a month: $70 of the owner's $100 of API credits.
pub const POOL_PER_MONTH: Micros = 70 * DOLLAR;
/// The days a month's pool is spread over.
pub const DAYS_A_MONTH: i64 = 30;
/// The most one turn may spend: $0.40.
pub const MOST_A_TURN: Micros = 400_000;
/// Less than this left for today, and a member cannot start a turn.
pub const LEAST_A_TURN: Micros = 10_000;

const DAY_MS: Millis = 86_400_000;

/// The pool for one day.
#[must_use]
pub const fn pool_a_day(pool_per_month: Micros) -> Micros {
    pool_per_month / DAYS_A_MONTH
}

/// A member's budget for one day: its share of the day's pool.
#[must_use]
pub const fn member_a_day(pool_per_month: Micros, share_percent: u8) -> Micros {
    pool_a_day(pool_per_month) * share_percent as Micros / 100
}

/// The day a moment falls on, as days since the epoch, by the owner's clock.
#[must_use]
pub const fn day_of(at: Millis, offset_minutes: i64) -> i64 {
    (at + offset_minutes * 60_000).div_euclid(DAY_MS)
}

/// What one turn can have reserved, before it starts: the per-turn cap, or what is left of
/// the member's day or the pool's day if that is less. `None` when less than a turn is left.
#[must_use]
pub fn reservation(member_left: Micros, pool_left: Micros) -> Option<Micros> {
    let amount = MOST_A_TURN.min(member_left).min(pool_left);
    (amount >= LEAST_A_TURN).then_some(amount)
}

/// Whether a running turn has gone past what it may spend: what it reserved.
#[must_use]
pub const fn crossed(spent: Micros, reserved: Micros) -> bool {
    spent > reserved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_is_split_by_share_per_day() {
        assert_eq!(pool_a_day(POOL_PER_MONTH), 2_333_333);
        assert_eq!(
            member_a_day(POOL_PER_MONTH, 40),
            933_333,
            "the developer's $0.93"
        );
        assert_eq!(
            member_a_day(POOL_PER_MONTH, 15),
            349_999,
            "the researcher's $0.35"
        );
    }

    #[test]
    fn a_turn_reserves_at_most_forty_cents_and_never_more_than_is_left() {
        assert_eq!(reservation(933_333, 2_333_333), Some(MOST_A_TURN));
        assert_eq!(reservation(120_000, 2_333_333), Some(120_000));
        assert_eq!(reservation(933_333, 50_000), Some(50_000));
        assert_eq!(
            reservation(9_999, 2_333_333),
            None,
            "less than a turn is left"
        );
    }

    #[test]
    fn a_turn_crosses_its_limit_by_spending_more_than_it_reserved() {
        assert!(!crossed(400_000, 400_000));
        assert!(crossed(400_001, 400_000));
    }

    #[test]
    fn the_day_is_the_owners() {
        // 03:00 UTC on 2026-10-17 is still the 16th five hours west.
        let at = 1_792_206_000_000;
        assert_eq!(day_of(at, 0) - day_of(at, -300), 1);
    }
}
