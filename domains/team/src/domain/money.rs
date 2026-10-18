//! Money: the team pool, each member's share of it per day, and what one turn may spend.
//!
//! The day's pool is shared only among the members with a paid brain, by their roles'
//! weights (ADR 0111): a member on the house's model costs nothing, takes no share, and is
//! never turned away for money.
//!
//! Amounts are millionths of a dollar, so a model's per-token price adds up without rounding.

use super::brain::Brain;
use super::work::{Millis, Role};

/// Millionths of a dollar.
pub type Micros = i64;

/// One dollar.
pub const DOLLAR: Micros = 1_000_000;
/// One cent: the least a turn can be given, and the unit the owner's downtime allowance is in.
pub const CENT: Micros = DOLLAR / 100;
/// The team pool for a month: $70 of the owner's $100 of API credits.
pub const POOL_PER_MONTH: Micros = 70 * DOLLAR;
/// The days a month's pool is spread over.
pub const DAYS_A_MONTH: i64 = 30;
/// The most one turn may spend: $0.40.
pub const MOST_A_TURN: Micros = 400_000;
/// The most one of the CEO's turns may spend: $1.00 (ADR 0111).
pub const CEO_MOST_A_TURN: Micros = DOLLAR;
/// Less than this left for today, and a member cannot start a turn.
pub const LEAST_A_TURN: Micros = 10_000;

/// One day, in milliseconds.
pub const DAY_MS: Millis = 86_400_000;

/// The pool for one day.
#[must_use]
pub const fn pool_a_day(pool_per_month: Micros) -> Micros {
    pool_per_month / DAYS_A_MONTH
}

/// A member's claim on the pool (ADR 0113, amended): its role's weight times its brain's price
/// class ([`Brain::price_class`]), so two members on Haiku do not dilute the lead on Sonnet as
/// if they cost what it does. Nothing for a member on the house's model.
#[must_use]
pub const fn claim(weight: u8, brain: Brain) -> i64 {
    weight as i64 * brain.price_class()
}

/// A paid member's budget for one day: its claim's part of the day's pool, shared among the
/// paid members' claims (`paid`, its own among them). Nothing when there is no claim.
#[must_use]
pub const fn member_a_day(pool_per_month: Micros, claim: i64, paid: i64) -> Micros {
    share_of_day(pool_a_day(pool_per_month), claim, paid)
}

/// A claim's part of a given day's pool (the base day, or the base day and the owner's top-up,
/// ADR 0113 rule 6), shared among the paid members' claims.
#[must_use]
pub const fn share_of_day(pool_day: Micros, claim: i64, paid: i64) -> Micros {
    if paid <= 0 {
        return 0;
    }
    pool_day * claim / paid
}

/// The most the owner may top the pool up in one owner's day, in all: three times the base
/// pool day (ADR 0113 rule 6).
#[must_use]
pub const fn most_top_up(pool_per_month: Micros) -> Micros {
    pool_a_day(pool_per_month) * 3
}

/// The most one turn of a member in `role` may spend.
#[must_use]
pub const fn most_a_turn(role: Role) -> Micros {
    match role {
        Role::Ceo => CEO_MOST_A_TURN,
        _ => MOST_A_TURN,
    }
}

/// The day a moment falls on, as days since the epoch, by the owner's clock.
#[must_use]
pub const fn day_of(at: Millis, offset_minutes: i64) -> i64 {
    (at + offset_minutes * 60_000).div_euclid(DAY_MS)
}

/// The moment a day (as [`day_of`] counts them) begins, by the owner's clock.
#[must_use]
pub const fn day_start(day: i64, offset_minutes: i64) -> Millis {
    day * DAY_MS - offset_minutes * 60_000
}

/// What the owner is offered to give a member that is out for the day, unless the pool's day has
/// less left: $0.50.
pub const ALLOWANCE_OFFER: Micros = 500_000;

/// What a card offers a member out for the day: [`ALLOWANCE_OFFER`], or what is left of the
/// pool's day if that is less.
#[must_use]
pub fn allowance_offer(pool_left: Micros) -> Micros {
    ALLOWANCE_OFFER.min(pool_left)
}

/// What a paid member may draw on today (ADR 0113, amended twice): its own share is its first
/// claim, and it may borrow at most one more share of what the others left, never more.
///
/// With `share` its share for the day, `spent` its own spend today (the owner's asking left
/// out), `others_left` each other paid member's share less its spend (one past its own share
/// lends nothing), and `allowance` what the owner gave it for today:
///
/// `may_draw = share + min(share, sum(max(0, others_left))) - spent + allowance`
///
/// so it may spend while `spent < share + min(share, others unused)`, and a member at twice its
/// share is out. The pool's day stays the hard ceiling: that is [`reservation`]'s.
#[must_use]
pub fn may_draw(
    share: Micros,
    spent: Micros,
    others_left: impl IntoIterator<Item = Micros>,
    allowance: Micros,
) -> Micros {
    let unused = others_left
        .into_iter()
        .fold(0, |sum: Micros, left| sum.saturating_add(left.max(0)));
    share
        .saturating_add(share.min(unused))
        .saturating_sub(spent)
        .saturating_add(allowance)
}

/// What one turn can have reserved, before it starts: the per-turn cap (`most`, the role's),
/// or what is left of the member's day or the pool's day if that is less. `None` when less
/// than a turn is left.
#[must_use]
pub fn reservation(member_left: Micros, pool_left: Micros, most: Micros) -> Option<Micros> {
    let amount = most.min(member_left).min(pool_left);
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
    fn the_pool_is_split_per_day_among_the_paid_by_their_weights() {
        assert_eq!(pool_a_day(POOL_PER_MONTH), 2_333_333);
        // Every member paid: the weights add up to a hundred, as the plan's shares do.
        assert_eq!(
            member_a_day(POOL_PER_MONTH, 40, 100),
            933_333,
            "the developer's $0.93"
        );
        assert_eq!(
            member_a_day(POOL_PER_MONTH, 15, 100),
            349_999,
            "the researcher's $0.35"
        );
        // Only the CEO and the lead paid: they split the day by 35 to 25.
        let (ceo, lead) = (
            member_a_day(POOL_PER_MONTH, i64::from(Role::Ceo.share()), 60),
            member_a_day(POOL_PER_MONTH, i64::from(Role::Lead.share()), 60),
        );
        assert_eq!((ceo, lead), (1_361_110, 972_222), "$1.36 and $0.97");
        assert!(ceo + lead <= pool_a_day(POOL_PER_MONTH));
        assert_eq!(
            member_a_day(POOL_PER_MONTH, 25, 0),
            0,
            "no weight, no share"
        );
    }

    #[test]
    fn a_members_claim_is_its_weight_times_its_brains_price_class() {
        // The live team: Mara on Opus, Ada and Kit on Sonnet, Cy, Rae and Zed on Haiku.
        let team = [
            claim(Role::Ceo.share(), Brain::Opus),
            claim(Role::Lead.share(), Brain::Sonnet),
            claim(Role::Lead.share(), Brain::Sonnet),
            claim(Role::Researcher.share(), Brain::Haiku),
            claim(Role::Developer.share(), Brain::Haiku),
            claim(Role::Developer.share(), Brain::Haiku),
        ];
        let paid: i64 = team.iter().sum();
        let share = |c: i64| member_a_day(POOL_PER_MONTH, c, paid);
        assert!(
            share(team[1]) > share(team[4]),
            "the Sonnet lead's share exceeds a Haiku developer's"
        );
        assert!(team.iter().map(|c| share(*c)).sum::<i64>() <= pool_a_day(POOL_PER_MONTH));
        assert_eq!(claim(Role::Developer.share(), Brain::Local), 0);
    }

    #[test]
    fn a_turn_reserves_at_most_its_roles_cap_and_never_more_than_is_left() {
        let most = most_a_turn(Role::Lead);
        assert_eq!(most, MOST_A_TURN);
        assert_eq!(reservation(933_333, 2_333_333, most), Some(MOST_A_TURN));
        assert_eq!(reservation(120_000, 2_333_333, most), Some(120_000));
        assert_eq!(reservation(933_333, 50_000, most), Some(50_000));
        assert_eq!(
            reservation(9_999, 2_333_333, most),
            None,
            "less than a turn is left"
        );
        // The CEO's turns may spend a dollar each (ADR 0111); nobody else's more than 40 cents.
        assert_eq!(most_a_turn(Role::Ceo), DOLLAR);
        for role in [Role::Developer, Role::Reviewer, Role::Researcher] {
            assert_eq!(most_a_turn(role), MOST_A_TURN);
        }
        assert_eq!(
            reservation(1_361_110, 2_333_333, most_a_turn(Role::Ceo)),
            Some(DOLLAR)
        );
    }

    #[test]
    fn a_member_may_borrow_one_more_share_and_no_further() {
        // Within its share ($0.30 of $0.50 spent): what is left of it, and one more share
        // besides, when the others have that much unused.
        assert_eq!(may_draw(500_000, 300_000, [900_000, 800_000], 0), 700_000);
        // Past its share by $0.05: it has borrowed $0.05 of its one extra share.
        assert_eq!(may_draw(500_000, 550_000, [900_000, 800_000], 0), 450_000);
        // The others' unused is the lesser of the two: it borrows only what they left.
        assert_eq!(may_draw(500_000, 550_000, [100_000, 50_000], 0), 100_000);
        // A member past its own share lends nothing.
        assert_eq!(may_draw(500_000, 550_000, [-100_000, 200_000], 0), 150_000);
        // At twice its share it is out, however much the others left.
        assert_eq!(may_draw(500_000, 1_000_000, [900_000], 0), 0);
        assert!(may_draw(500_000, 1_000_000, [900_000], 0) < LEAST_A_TURN);
        // The owner's allowance for the day is added to what it may draw.
        assert_eq!(may_draw(500_000, 1_000_000, [900_000], 50_000), 50_000);
        // And what it may draw is still held to the turn's cap and the pool's day.
        let most = most_a_turn(Role::Lead);
        assert_eq!(
            reservation(may_draw(500_000, 550_000, [2_000_000], 0), 2_333_333, most),
            Some(most)
        );
        assert_eq!(
            reservation(may_draw(500_000, 550_000, [2_000_000], 0), 5_000, most),
            None
        );
    }

    #[test]
    fn the_owners_offer_is_fifty_cents_or_what_the_pools_day_has_left() {
        assert_eq!(allowance_offer(2_000_000), 500_000);
        assert_eq!(allowance_offer(58_824), 58_824);
    }

    #[test]
    fn a_top_up_scales_every_share_by_the_pool_days_growth_and_is_held_to_three_days() {
        let day = pool_a_day(POOL_PER_MONTH);
        let share = |pool_day| share_of_day(pool_day, 40, 100);
        assert_eq!(share(day), member_a_day(POOL_PER_MONTH, 40, 100));
        assert_eq!(
            share(2 * day),
            2 * share(day),
            "doubled pool, doubled share"
        );
        assert_eq!(share_of_day(day, 25, 0), 0);
        assert_eq!(most_top_up(POOL_PER_MONTH), 3 * day);
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

    #[test]
    fn a_day_starts_where_the_owners_clock_says_and_holds_its_moments() {
        let at = 1_792_206_000_000;
        for offset in [0, -300, 120] {
            let day = day_of(at, offset);
            let start = day_start(day, offset);
            assert!(start <= at && at < start + DAY_MS, "offset {offset}");
            assert_eq!(day_of(start, offset), day);
            assert_eq!(day_of(start - 1, offset), day - 1);
        }
    }
}
