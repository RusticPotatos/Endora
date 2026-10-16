//! What earlier bills from the same sender say about a new one (ADR 0097). Evidence, never a
//! verdict: a bill like the last one is routine, not proof it is real.

/// A bill the same sender sent before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierBill {
    /// When it arrived.
    pub at_ms: i64,
    /// What it asked for, if it said.
    pub amount: Option<String>,
}

/// How a bill compares with the sender's earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillHistory {
    /// How many bills they sent before this one.
    pub times: usize,
    /// When the last of them arrived.
    pub last_at_ms: i64,
    /// What the last of them asked for.
    pub last_amount: Option<String>,
    /// This one asks for the same as the last: routine.
    pub same_amount: bool,
}

/// An amount in cents, however it was written (`20`, `20.00`, `75.270`).
fn cents(amount: &str) -> Option<i64> {
    let (whole, fraction) = amount.trim().split_once('.').unwrap_or((amount.trim(), ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut fraction = fraction.trim_end_matches('0').to_owned();
    if fraction.len() > 2 {
        return None;
    }
    while fraction.len() < 2 {
        fraction.push('0');
    }
    Some(whole.parse::<i64>().ok()? * 100 + fraction.parse::<i64>().ok()?)
}

/// The sender's bills before `at_ms`, or nothing if this is the first.
#[must_use]
pub fn bill_history(
    at_ms: i64,
    amount: Option<&str>,
    earlier: &[EarlierBill],
) -> Option<BillHistory> {
    let before: Vec<&EarlierBill> = earlier.iter().filter(|b| b.at_ms < at_ms).collect();
    let last = before.iter().max_by_key(|b| b.at_ms)?;
    let same_amount = match (
        amount.and_then(cents),
        last.amount.as_deref().and_then(cents),
    ) {
        (Some(this), Some(that)) => this == that,
        _ => false,
    };
    Some(BillHistory {
        times: before.len(),
        last_at_ms: last.at_ms,
        last_amount: last.amount.clone(),
        same_amount,
    })
}
