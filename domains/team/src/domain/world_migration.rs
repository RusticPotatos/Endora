//! A page's timeline line, and the calendar arithmetic the house page's dates are read with. The
//! move of the old house log and lessons into the world (ADR 0122) is done, and went with the
//! world's curation (ADR 0142).

use super::work::Millis;

/// One line for a page's timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineLine {
    /// The page's slug.
    pub page: String,
    /// When.
    pub at: Millis,
    /// Who wrote it.
    pub by: String,
    /// What it says.
    pub line: String,
}

/// Days since the Unix epoch of a civil date (Howard Hinnant's days-from-civil).
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_are_counted_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    }
}
