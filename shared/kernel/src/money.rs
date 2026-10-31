//! Dollars as the owner reads and writes them: one way to say an amount kept in millionths of a
//! dollar, and one way to read what the owner typed, for every context that shows money (the
//! team's projects and allowances, the node's credits).

/// One dollar, in millionths.
pub const DOLLAR: i64 = 1_000_000;

/// An amount in millionths of a dollar as dollars and cents, rounded to the nearest cent:
/// `$12.50`, `-$0.50`.
#[must_use]
pub fn dollars(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let cents = (micros.unsigned_abs() + 5_000) / 10_000;
    format!("{sign}${}.{:02}", cents / 100, cents % 100)
}

/// An amount the owner wrote in dollars (`25`, `$12.50`), in millionths, if it reads as one
/// above nothing, to the cent.
#[must_use]
pub fn read_dollars(text: &str) -> Option<i64> {
    let text = text.trim().trim_start_matches('$').trim();
    let (whole, cents) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && cents.is_empty() {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    if cents.len() > 2 || !cents.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let cents: i64 = format!("{cents:0<2}").parse().ok()?;
    let amount = whole.checked_mul(DOLLAR)?.checked_add(cents * 10_000)?;
    (amount > 0).then_some(amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollars_are_said_to_the_cent_whatever_their_sign() {
        assert_eq!(dollars(4_210_000), "$4.21");
        assert_eq!(dollars(4_999), "$0.00");
        assert_eq!(dollars(5_000), "$0.01");
        assert_eq!(dollars(-500_000), "-$0.50");
        assert_eq!(read_dollars(" $12.5 "), Some(12_500_000));
        for bad in ["", "$", "0", "-3", "1.234", "ten", "1.x"] {
            assert_eq!(read_dollars(bad), None, "{bad:?}");
        }
    }
}
