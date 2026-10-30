//! How much the owner has left with each paid model provider (the owner, 2026-10-30): DeepSeek
//! says its balance itself; Anthropic does not, so its balance is the figure the owner last read
//! from Anthropic's Console, less what Endora has paid it since, which the relay meters to the
//! micro-dollar. Below a threshold the owner is told, once, to top up. These are the rules; the
//! node reads the balance, keeps the figures and tells the owner.

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
}
