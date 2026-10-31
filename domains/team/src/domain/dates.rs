//! Days as the owner reads them: a day's date, and whether a word names a month. The lessons
//! these began with (a lead's line in a goal's notes) retired with the leads (ADR 0142).

/// The months' short names, January first.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The day of the month and the month's short name of `day` (days since the Unix epoch).
pub(crate) fn day_and_month(day: i64) -> (u32, &'static str) {
    let (_, month, d) = civil(day);
    (d, MONTHS[(month as usize + 11) % 12])
}

/// The year, the month (1 to 12) and the day of the month of `day` (days since the Unix
/// epoch), by Howard Hinnant's civil-from-days, for the proleptic Gregorian calendar.
pub(crate) fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(m <= 2);
    (
        year,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// Whether `word` names a month, as people write them: `Oct`, `October`, `oct.`.
pub(crate) fn is_month(word: &str) -> bool {
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let word = word.trim_end_matches('.').to_ascii_lowercase();
    word.len() >= 3 && NAMES.iter().any(|name| name.starts_with(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_reads_as_its_date_in_the_proleptic_gregorian_calendar() {
        assert_eq!(day_and_month(0), (1, "Jan"));
        assert_eq!(day_and_month(-1), (31, "Dec"));
        assert_eq!(day_and_month(19_782), (29, "Feb"), "2024 was a leap year");
        assert_eq!(civil(20_744), (2026, 10, 18));
        assert!(is_month("Oct") && is_month("october") && is_month("oct."));
        assert!(!is_month("Oc") && !is_month("Octopus"));
    }
}
