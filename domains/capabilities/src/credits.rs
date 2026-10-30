//! How much the owner has left with each paid model provider (the owner, 2026-10-30): DeepSeek
//! says its balance itself; Anthropic does not, so its balance is the figure the owner last read
//! from Anthropic's Console, less what Endora has paid it since, which the relay meters to the
//! micro-dollar. A plan's monthly grant of API credit (Claude Max's) is whole again on the day
//! the plan renews, and is spent before purchased credit ([`Pots`]). Below a threshold the owner
//! is told, once, to top up. These are the rules; the node reads the balance, keeps the figures
//! and tells the owner.

use serde_json::Value;

/// What DeepSeek says is left, in one currency.
#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    /// What is left in all, granted and topped up.
    pub total: f64,
    /// Its currency, as DeepSeek names it: `USD` or `CNY`.
    pub currency: String,
    /// Whether DeepSeek will still take calls.
    pub available: bool,
}

/// DeepSeek's answer to `GET /user/balance`, read: the dollar balance if there is one, else the
/// first. `None` for an answer that holds no balance.
#[must_use]
pub fn deepseek_balance(answer: &Value) -> Option<Balance> {
    let infos = answer["balance_infos"].as_array()?;
    let info = infos
        .iter()
        .find(|i| i["currency"] == "USD")
        .or_else(|| infos.first())?;
    let total = info["total_balance"]
        .as_str()
        .and_then(|t| t.trim().parse::<f64>().ok())
        .or_else(|| info["total_balance"].as_f64())?;
    Some(Balance {
        total,
        currency: info["currency"].as_str().unwrap_or("USD").to_owned(),
        available: answer["is_available"].as_bool().unwrap_or(total > 0.0),
    })
}

/// What is left with Anthropic, in micros: the balance the owner read (`set`), less what Endora
/// has paid since (`spent_now - spent_at_set`). Never below nothing.
#[must_use]
pub const fn claude_left(set: i64, spent_at_set: i64, spent_now: i64) -> i64 {
    let since = spent_now - spent_at_set;
    let left = set - if since > 0 { since } else { 0 };
    if left > 0 { left } else { 0 }
}

/// Claude's credit as two pots, in micros. The model (the owner, 2026-10-30): a plan such as
/// Claude Max grants API credit each billing cycle, which expires at the cycle's end and is
/// spent before purchased credit. So what Endora pays comes off the `grant` first, then off
/// `purchased`; at renewal the grant is whole again ([`renew`]) and what is left of the
/// purchased credit carries over. Without a grant all of it is one pot and the order is moot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pots {
    /// What is left of this cycle's grant.
    pub grant: i64,
    /// What is left of the credit the owner bought.
    pub purchased: i64,
}

impl Pots {
    /// Both together: what the Console would show.
    #[must_use]
    pub const fn left(self) -> i64 {
        self.grant + self.purchased
    }
}

/// Claude's pots now: `set` in all when the owner last said it (or a renewal set it), of which
/// `purchased_at_set` was purchased credit (held to `set`), less what Endora has paid since
/// (`spent_now - spent_at_set`), taken from the grant first. Never below nothing.
#[must_use]
pub const fn claude_pots(
    set: i64,
    purchased_at_set: i64,
    spent_at_set: i64,
    spent_now: i64,
) -> Pots {
    let set = if set > 0 { set } else { 0 };
    let purchased = if purchased_at_set < 0 {
        0
    } else if purchased_at_set > set {
        set
    } else {
        purchased_at_set
    };
    let grant = set - purchased;
    let since = spent_now - spent_at_set;
    let since = if since > 0 { since } else { 0 };
    if since <= grant {
        return Pots {
            grant: grant - since,
            purchased,
        };
    }
    let rest = purchased - (since - grant);
    Pots {
        grant: 0,
        purchased: if rest > 0 { rest } else { 0 },
    }
}

/// The figures a renewal starts from, `(set, purchased_at_set)`: the whole `grant` again, plus
/// what was left of the purchased credit; what was left of the old grant has expired.
#[must_use]
pub const fn renew(grant: i64, before: Pots) -> (i64, i64) {
    (grant + before.purchased, before.purchased)
}

/// A day, in milliseconds.
pub const DAY_MS: i64 = 86_400_000;

/// The days from 1970-01-01 to a civil date (proleptic Gregorian; Howard Hinnant's algorithm).
#[must_use]
pub const fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of a day counted from 1970-01-01.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// How many days a month has.
#[must_use]
pub const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The plan's renewal in a month: `renews_on` (1–31), or the month's last day when it is
/// shorter (renewing on the 31st, February renews on the 28th or 29th).
const fn renewal_in(year: i64, month: u32, renews_on: u32) -> i64 {
    let last = days_in_month(year, month);
    days_from_civil(year, month, if renews_on < last { renews_on } else { last })
}

/// The day (from 1970-01-01) of the latest renewal on or before `today`.
#[must_use]
pub const fn last_renewal(today: i64, renews_on: u32) -> i64 {
    let (year, month, _) = civil_from_days(today);
    let this = renewal_in(year, month, renews_on);
    if this <= today {
        return this;
    }
    if month == 1 {
        return renewal_in(year - 1, 12, renews_on);
    }
    renewal_in(year, month - 1, renews_on)
}

/// The day (from 1970-01-01) of the first renewal after `today`.
#[must_use]
pub const fn next_renewal(today: i64, renews_on: u32) -> i64 {
    let (year, month, _) = civil_from_days(today);
    let this = renewal_in(year, month, renews_on);
    if this > today {
        return this;
    }
    if month == 12 {
        return renewal_in(year + 1, 1, renews_on);
    }
    renewal_in(year, month + 1, renews_on)
}

/// Whether the plan has renewed since `since_ms`: the owner's midnight starting the latest
/// renewal day on or before `now_ms` came after it. `offset_minutes` is the owner's offset from
/// UTC now (a renewal in the hour a clock changes may land an hour off).
#[must_use]
pub const fn renewal_due(since_ms: i64, now_ms: i64, offset_minutes: i64, renews_on: u32) -> bool {
    let offset_ms = offset_minutes * 60_000;
    let today = (now_ms + offset_ms).div_euclid(DAY_MS);
    let renewed_at = last_renewal(today, renews_on) * DAY_MS - offset_ms;
    since_ms < renewed_at
}

/// What DeepSeek's balance must fall under before the owner is told by default: a dollar.
pub const DEEPSEEK_LOW: i64 = 1_000_000;

/// What Anthropic's must fall under by default: five dollars.
pub const CLAUDE_LOW: i64 = 5_000_000;

/// Whether to tell the owner now: the balance is below the threshold and they have not been
/// told since it last was above it. Returns the new "told" mark too, cleared once the balance
/// is above the threshold again, so a top-up re-arms the warning.
#[must_use]
pub const fn warn(left: i64, low: i64, told: bool) -> (bool, bool) {
    if left >= low {
        return (false, false);
    }
    (!told, true)
}

/// Dollars from micros, for the owner: `$4.21`.
#[must_use]
pub fn dollars(micros: i64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let d = micros as f64 / 1_000_000.0;
    format!("${d:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deepseeks_balance_is_read_in_dollars_when_it_has_them() {
        let answer = json!({ "is_available": true, "balance_infos": [
            { "currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00" },
            { "currency": "USD", "total_balance": "4.21", "granted_balance": "0.00", "topped_up_balance": "4.21" },
        ] });
        let read = deepseek_balance(&answer).unwrap();
        assert!((read.total - 4.21).abs() < 1e-9);
        assert_eq!(read.currency, "USD");
        assert!(read.available);
        let only_cny = json!({ "is_available": false, "balance_infos": [
            { "currency": "CNY", "total_balance": "0.00" } ] });
        let read = deepseek_balance(&only_cny).unwrap();
        assert_eq!(read.currency, "CNY");
        assert!(!read.available);
        assert_eq!(deepseek_balance(&json!({ "error": "unauthorized" })), None);
    }

    #[test]
    fn claudes_balance_is_the_owners_figure_less_what_was_paid_since() {
        assert_eq!(claude_left(10_000_000, 3_000_000, 3_000_000), 10_000_000);
        assert_eq!(claude_left(10_000_000, 3_000_000, 5_500_000), 7_500_000);
        assert_eq!(
            claude_left(1_000_000, 0, 9_000_000),
            0,
            "never below nothing"
        );
    }

    #[test]
    fn the_owner_is_told_once_when_a_balance_falls_low_and_again_after_a_top_up() {
        assert_eq!(warn(6_000_000, CLAUDE_LOW, false), (false, false));
        assert_eq!(warn(4_000_000, CLAUDE_LOW, false), (true, true), "told");
        assert_eq!(
            warn(3_000_000, CLAUDE_LOW, true),
            (false, true),
            "not again"
        );
        assert_eq!(
            warn(9_000_000, CLAUDE_LOW, true),
            (false, false),
            "re-armed"
        );
        assert_eq!(warn(500_000, DEEPSEEK_LOW, false), (true, true));
        assert_eq!(dollars(4_210_000), "$4.21");
    }

    #[test]
    fn a_plan_renews_on_its_day_or_the_months_last_when_the_month_is_shorter() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2028, 2), 29, "a leap year");
        assert_eq!(days_in_month(2100, 2), 28, "a century is not");
        assert_eq!(days_in_month(2000, 2), 29, "but every fourth century is");
        assert_eq!(days_in_month(2026, 4), 30);
        let day = |y, m, d| days_from_civil(y, m, d);
        assert_eq!(civil_from_days(day(2026, 10, 30)), (2026, 10, 30));
        assert_eq!(day(1970, 1, 1), 0);
        // Renewing on the 31st: in February it is the 28th, in April the 30th.
        assert_eq!(last_renewal(day(2026, 3, 1), 31), day(2026, 2, 28));
        assert_eq!(last_renewal(day(2026, 2, 27), 31), day(2026, 1, 31));
        assert_eq!(last_renewal(day(2026, 4, 30), 31), day(2026, 4, 30));
        assert_eq!(last_renewal(day(2028, 2, 29), 30), day(2028, 2, 29));
        // On the day itself it has renewed; the day before, the last renewal was a month ago.
        assert_eq!(last_renewal(day(2026, 10, 15), 15), day(2026, 10, 15));
        assert_eq!(last_renewal(day(2026, 10, 14), 15), day(2026, 9, 15));
        // Across a year's end.
        assert_eq!(last_renewal(day(2027, 1, 3), 20), day(2026, 12, 20));
        assert_eq!(next_renewal(day(2026, 12, 20), 20), day(2027, 1, 20));
        assert_eq!(next_renewal(day(2026, 1, 31), 31), day(2026, 2, 28));
        assert_eq!(next_renewal(day(2026, 2, 28), 31), day(2026, 3, 31));
        assert_eq!(next_renewal(day(2026, 2, 10), 29), day(2026, 2, 28));
    }

    #[test]
    fn a_renewal_is_due_once_the_owners_midnight_on_the_day_has_passed() {
        let midnight = days_from_civil(2026, 11, 15) * DAY_MS;
        let before = midnight - 3_600_000;
        // In UTC: due from midnight, not an hour before.
        assert!(!renewal_due(before - DAY_MS, before, 0, 15));
        assert!(renewal_due(before, midnight, 0, 15));
        assert!(!renewal_due(midnight, midnight + 5 * DAY_MS, 0, 15), "once");
        // Five hours behind UTC, the owner's midnight is 05:00 UTC.
        assert!(!renewal_due(before, midnight + 4 * 3_600_000, -300, 15));
        assert!(renewal_due(before, midnight + 5 * 3_600_000, -300, 15));
        // Months missed renew once.
        assert!(renewal_due(
            before - 90 * DAY_MS,
            before + 2 * 3_600_000,
            0,
            15
        ));
    }

    #[test]
    fn spending_takes_the_grant_first_then_purchased_credit() {
        // $100 in all, $30 of it purchased: the first $70 spent is the grant's.
        let pots = claude_pots(100_000_000, 30_000_000, 0, 50_000_000);
        assert_eq!((pots.grant, pots.purchased), (20_000_000, 30_000_000));
        let pots = claude_pots(100_000_000, 30_000_000, 0, 80_000_000);
        assert_eq!((pots.grant, pots.purchased), (0, 20_000_000));
        let pots = claude_pots(100_000_000, 30_000_000, 0, 130_000_000);
        assert_eq!((pots.grant, pots.purchased), (0, 0), "never below nothing");
        assert_eq!(
            claude_pots(100_000_000, 30_000_000, 10, 10).left(),
            100_000_000
        );
        // A purchased figure above the total is held to it.
        let pots = claude_pots(10_000_000, 30_000_000, 0, 0);
        assert_eq!((pots.grant, pots.purchased), (0, 10_000_000));
        // At renewal the grant is whole again and what was purchased carries over.
        let after = renew(100_000_000, pots_after(80_000_000));
        assert_eq!(after, (120_000_000, 20_000_000));
    }

    fn pots_after(spent: i64) -> Pots {
        claude_pots(100_000_000, 30_000_000, 0, spent)
    }
}
