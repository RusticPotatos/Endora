//! Today's base pool, sized from the Claude credit (ADR 0113, amended 2026-10-31).
//!
//! The owner's plan grants Claude API credit each month, and what is left of it is lost on the
//! day the plan renews. So the team's pool for a day is what that grant has left spread over
//! the days until it renews, counting today, and never less than the floor (the pool a month
//! over [`DAYS_A_MONTH`] days, as before) nor more than three times the floor (the same bound
//! as the owner's top-ups). Credit the owner bought does not expire, so it never raises the
//! pool. The floor's part for brains that are not Claude (DeepSeek) is kept on top, since the
//! grant pays only for Claude.
//!
//! Pure arithmetic: the node says the figures, the engine keeps them for the day, and this
//! decides what they come to.

use super::dates::civil;
use super::money::{DAYS_A_MONTH, Micros, pool_a_day};
use super::projects::dollars;
use super::world_migration::days_from_civil;

/// What the node said about the Claude credit on one owner's day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditWord {
    /// The owner has set no plan grant: there is nothing that expires.
    NoGrant,
    /// What is left of this cycle's grant, and the day the plan next renews.
    Grant {
        /// What is left of the grant, in micros. Bought credit is never in it.
        left: Micros,
        /// The day the plan next renews, as days since 1970-01-01 on the owner's calendar.
        renews: i64,
    },
}

impl CreditWord {
    /// As kept in a setting: `none`, or `<left>,<renews>`.
    #[must_use]
    pub fn kept(self) -> String {
        match self {
            Self::NoGrant => "none".to_owned(),
            Self::Grant { left, renews } => format!("{left},{renews}"),
        }
    }

    /// Read back from [`Self::kept`]; `None` for anything else.
    #[must_use]
    pub fn read(kept: &str) -> Option<Self> {
        if kept == "none" {
            return Some(Self::NoGrant);
        }
        let (left, renews) = kept.split_once(',')?;
        Some(Self::Grant {
            left: left.parse().ok()?,
            renews: renews.parse().ok()?,
        })
    }
}

/// A date as the node writes it, `YYYY-MM-DD`, as days since 1970-01-01; `None` when it is not
/// one.
#[must_use]
pub fn day_of_date(date: &str) -> Option<i64> {
    let mut parts = date.trim().splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Why today's base pool is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    /// Nothing was heard about the credit today (or what was heard was out of date): the floor.
    Unread,
    /// The owner has no plan grant: the floor.
    NoGrant,
    /// The grant spread over the days left gives less than the floor: the floor.
    FloorIsMore,
    /// Sized from the grant.
    Credit,
    /// The grant would give more than three times the floor: the ceiling.
    Ceiling,
}

impl Sizing {
    /// Its stable name, for the snapshot.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::NoGrant => "no_grant",
            Self::FloorIsMore => "floor",
            Self::Credit => "credit",
            Self::Ceiling => "ceiling",
        }
    }
}

/// Today's base pool and how it was sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolDay {
    /// Today's base pool, before the owner's top-up.
    pub base: Micros,
    /// The floor: the pool a month over [`DAYS_A_MONTH`] days.
    pub floor: Micros,
    /// The most the base may be: three times the floor.
    pub ceiling: Micros,
    /// Why.
    pub sizing: Sizing,
    /// The pool a month the floor is made from.
    per_month: Micros,
    /// The grant left, the days until it renews (today counted) and the day of the month it
    /// renews on, when the grant was heard today.
    grant: Option<(Micros, i64, u32)>,
}

/// The most the base pool may be, in floors: the bound the owner already set on a day's top-ups
/// (ADR 0113 rule 6).
pub const CEILING_FLOORS: i64 = 3;

/// The days until the plan renews, counting today: renewing tomorrow leaves one.
#[must_use]
pub const fn days_to_renewal(today: i64, renews: i64) -> i64 {
    renews - today
}

/// Today's base pool (ADR 0113, amended 2026-10-31).
///
/// With `floor = pool_per_month / 30` and `others` the floor's part for brains that are not
/// Claude (their claims' part of the floor), when the grant was heard today:
///
/// `base = min(3 * floor, max(floor, grant_left / days_to_renewal + others))`
///
/// and the floor when it was not, when there is no grant, or when what was heard names a
/// renewal that is not after today.
#[must_use]
pub fn sized(
    pool_per_month: Micros,
    others: Micros,
    word: Option<CreditWord>,
    today: i64,
) -> PoolDay {
    let floor = pool_a_day(pool_per_month);
    let ceiling = floor * CEILING_FLOORS;
    let mut day = PoolDay {
        base: floor,
        floor,
        ceiling,
        sizing: Sizing::Unread,
        per_month: pool_per_month,
        grant: None,
    };
    let (left, renews) = match word {
        None => return day,
        Some(CreditWord::NoGrant) => {
            day.sizing = Sizing::NoGrant;
            return day;
        }
        Some(CreditWord::Grant { left, renews }) => (left.max(0), renews),
    };
    let days = days_to_renewal(today, renews);
    if days < 1 {
        return day;
    }
    day.grant = Some((left, days, civil(renews).2));
    let wanted = left / days + others.max(0);
    if wanted <= floor {
        day.sizing = Sizing::FloorIsMore;
        return day;
    }
    if wanted > ceiling {
        day.base = ceiling;
        day.sizing = Sizing::Ceiling;
        return day;
    }
    day.base = wanted;
    day.sizing = Sizing::Credit;
    day
}

/// 1st, 2nd, 3rd, 4th, 11th, 12th, 13th, 21st.
fn ordinal(n: u32) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

impl PoolDay {
    /// What the owner reads beside today's pool: which figure was used, and why.
    #[must_use]
    pub fn why(&self) -> String {
        let floor = format!(
            "the floor, {} a month over {DAYS_A_MONTH} days",
            dollars(self.per_month)
        );
        let credit = self.grant.map(|(left, days, on)| {
            format!(
                "{} left, {days} day{} to the {}",
                dollars(left),
                if days == 1 { "" } else { "s" },
                ordinal(on)
            )
        });
        match (self.sizing, credit) {
            (Sizing::Credit, Some(credit)) => format!("Sized from the Claude credit: {credit}."),
            (Sizing::Ceiling, Some(credit)) => format!(
                "Held at the ceiling, three times the floor: the Claude credit has {credit}."
            ),
            (Sizing::FloorIsMore, Some(credit)) => format!(
                "Sized from {floor}: the Claude credit has {credit}, less than the floor a day."
            ),
            (Sizing::NoGrant, _) => {
                format!("Sized from {floor}: no plan grant of Claude credit is set.")
            }
            _ => format!("Sized from {floor}: the Claude credit has not been heard today."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::money::DOLLAR;

    /// The house's pool: $95 a month, a floor of $3.17 a day.
    const POOL: Micros = 95 * DOLLAR;
    const FLOOR: Micros = 3_166_666;

    fn day(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d)
    }

    fn grant(left: Micros, renews: i64) -> Option<CreditWord> {
        Some(CreditWord::Grant { left, renews })
    }

    #[test]
    fn credit_that_would_expire_sizes_todays_pool() {
        // 31 October, $70.23 of the grant left, the plan renews on 18 November: 18 days.
        let today = day(2026, 10, 31);
        let renews = day(2026, 11, 18);
        assert_eq!(days_to_renewal(today, renews), 18);
        let pool = sized(POOL, 0, grant(70_230_000, renews), today);
        assert_eq!(pool.base, 3_901_666, "$70.23 / 18");
        assert_eq!(pool.sizing, Sizing::Credit);
        assert_eq!(
            pool.why(),
            "Sized from the Claude credit: $70.23 left, 18 days to the 18th."
        );
    }

    #[test]
    fn on_the_renewal_day_the_whole_grant_is_spread_over_the_month_to_come() {
        // 18 November: the grant is whole again, and the next renewal is 18 December.
        let today = day(2026, 11, 18);
        let pool = sized(POOL, 0, grant(100 * DOLLAR, day(2026, 12, 18)), today);
        assert_eq!(pool.base, 3_333_333, "$100 / 30");
        assert_eq!(pool.sizing, Sizing::Credit);
        // A word that names today (or before) as the renewal is out of date: the floor.
        let stale = sized(POOL, 0, grant(100 * DOLLAR, today), today);
        assert_eq!((stale.base, stale.sizing), (FLOOR, Sizing::Unread));
    }

    #[test]
    fn on_the_last_day_what_is_left_is_todays_and_the_ceiling_holds() {
        let today = day(2026, 11, 17);
        let renews = day(2026, 11, 18);
        let pool = sized(POOL, 0, grant(5 * DOLLAR, renews), today);
        assert_eq!((pool.base, pool.sizing), (5 * DOLLAR, Sizing::Credit));
        assert_eq!(
            pool.why(),
            "Sized from the Claude credit: $5.00 left, 1 day to the 18th."
        );
        // $60 left on the last day would be $60 today: held at three times the floor.
        let pool = sized(POOL, 0, grant(60 * DOLLAR, renews), today);
        assert_eq!(pool.base, 3 * FLOOR, "$9.50");
        assert_eq!(pool.ceiling, 3 * FLOOR);
        assert_eq!(pool.sizing, Sizing::Ceiling);
        assert_eq!(
            pool.why(),
            "Held at the ceiling, three times the floor: the Claude credit has $60.00 left, 1 day \
             to the 18th."
        );
    }

    #[test]
    fn with_no_grant_or_no_word_the_floor_as_before() {
        let today = day(2026, 10, 31);
        let none = sized(POOL, 0, Some(CreditWord::NoGrant), today);
        assert_eq!((none.base, none.sizing), (FLOOR, Sizing::NoGrant));
        assert_eq!(
            none.why(),
            "Sized from the floor, $95.00 a month over 30 days: no plan grant of Claude credit is \
             set."
        );
        let unread = sized(POOL, 0, None, today);
        assert_eq!((unread.base, unread.sizing), (FLOOR, Sizing::Unread));
        assert_eq!(
            unread.why(),
            "Sized from the floor, $95.00 a month over 30 days: the Claude credit has not been \
             heard today."
        );
    }

    #[test]
    fn credit_that_would_last_past_the_renewal_leaves_the_floor() {
        let today = day(2026, 10, 31);
        let pool = sized(POOL, 0, grant(20 * DOLLAR, day(2026, 11, 18)), today);
        assert_eq!((pool.base, pool.sizing), (FLOOR, Sizing::FloorIsMore));
        assert_eq!(
            pool.why(),
            "Sized from the floor, $95.00 a month over 30 days: the Claude credit has $20.00 left, \
             18 days to the 18th, less than the floor a day."
        );
        // Nothing left of the grant (only bought credit, which never expires): the floor.
        let spent = sized(POOL, 0, grant(0, day(2026, 11, 18)), today);
        assert_eq!((spent.base, spent.sizing), (FLOOR, Sizing::FloorIsMore));
    }

    #[test]
    fn the_floors_part_for_brains_that_are_not_claude_is_kept_on_top() {
        let today = day(2026, 10, 31);
        let renews = day(2026, 11, 18);
        // A tenth of the floor is DeepSeek's: the grant pays only for Claude.
        let others = FLOOR / 10;
        let pool = sized(POOL, others, grant(70_230_000, renews), today);
        assert_eq!(pool.base, 3_901_666 + others);
        // A grant that gives less than the floor's Claude part leaves the floor.
        let low = sized(POOL, others, grant(FLOOR * 18 * 9 / 10, renews), today);
        assert_eq!((low.base, low.sizing), (FLOOR, Sizing::FloorIsMore));
        // And the ceiling holds whatever is on top.
        let high = sized(POOL, others, grant(10 * DOLLAR * 18, renews), today);
        assert_eq!((high.base, high.sizing), (3 * FLOOR, Sizing::Ceiling));
    }

    #[test]
    fn the_word_is_kept_and_read_back_and_dates_are_read() {
        for word in [
            CreditWord::NoGrant,
            CreditWord::Grant {
                left: 70_230_000,
                renews: day(2026, 11, 18),
            },
        ] {
            assert_eq!(CreditWord::read(&word.kept()), Some(word));
        }
        assert_eq!(CreditWord::read("70,x"), None);
        assert_eq!(day_of_date("2026-11-18"), Some(day(2026, 11, 18)));
        assert_eq!(day_of_date("2026-13-01"), None);
        assert_eq!(day_of_date("18 November"), None);
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(23), "23rd");
    }
}
