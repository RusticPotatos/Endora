//! The tripwire against a runaway (ADR 0133), as a pure rule over when turns started.
//!
//! The scripted demo ran 1,539 turns in 3 h 40 min, and the owner alone could stop a team: the
//! plan's tripwires that pull the kill switch were never built. A team that starts more than
//! [`Thresholds::runaway_turns`](super::guards::Thresholds::runaway_turns) turns in any
//! [`Thresholds::runaway_window_ms`](super::guards::Thresholds::runaway_window_ms) is stopped by
//! HQ itself, with the evidence on the owner's desk; the owner resumes it.

use super::guards::THRESHOLDS;
use super::work::{MemberId, Millis};

/// What the tripwire saw when it tripped: the turns the team had started in its window, and who
/// started them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runaway {
    /// How many turns the team had started in the window.
    pub started: usize,
    /// How many each member started, most first (equal counts by member).
    pub by_member: Vec<(MemberId, usize)>,
}

/// Whether the team has started as many turns as it may in the tripwire's window, so that
/// starting one more would be a runaway (ADR 0133): `starts` are every turn started, by whom and
/// when; only those within the window before `now`, and after `since`, when the owner last
/// resumed the team, count. Asked as a turn is about to start: the one that would be the forty-first
/// is the one that trips the wire, and never starts.
#[must_use]
pub fn runaway(starts: &[(MemberId, Millis)], now: Millis, since: Millis) -> Option<Runaway> {
    let window = THRESHOLDS.runaway_window_ms;
    let mut by_member: Vec<(MemberId, usize)> = Vec::new();
    let mut started = 0;
    for (member, _) in starts
        .iter()
        .filter(|(_, at)| *at <= now && *at > since && now - *at < window)
    {
        started += 1;
        match by_member.iter_mut().find(|(m, _)| m == member) {
            Some(found) => found.1 += 1,
            None => by_member.push((*member, 1)),
        }
    }
    if started < THRESHOLDS.runaway_turns {
        return None;
    }
    by_member.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.0.cmp(&b.0.0)));
    Some(Runaway { started, by_member })
}

impl Runaway {
    /// Who started how many, as the owner reads it: `Dee 20, Linus 18`.
    #[must_use]
    pub fn said(&self, name: impl Fn(MemberId) -> String) -> String {
        self.by_member
            .iter()
            .map(|(member, turns)| format!("{} {turns}", name(*member)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Millis = 60_000;

    fn started(n: usize, member: i64, at: Millis) -> Vec<(MemberId, Millis)> {
        vec![(MemberId(member), at); n]
    }

    /// Forty turns in ten minutes is what a team may start; asked as the forty-first is about to,
    /// the wire trips. Thirty-nine is nothing.
    #[test]
    fn forty_starts_in_ten_minutes_are_the_most_and_the_forty_first_trips_the_wire() {
        let now = 100 * MINUTE;
        assert_eq!(runaway(&started(39, 1, now - MINUTE), now, 0), None);
        assert_eq!(
            runaway(&started(40, 1, now - MINUTE), now, 0),
            Some(Runaway {
                started: 40,
                by_member: vec![(MemberId(1), 40)]
            })
        );
        assert_eq!(runaway(&[], now, 0), None);
    }

    /// Only the last ten minutes count, to the millisecond, and only what came after the owner
    /// last resumed the team: the evidence of a runaway the owner has dealt with is not a runaway.
    #[test]
    fn only_starts_inside_the_window_and_after_the_owner_resumed_count() {
        let now = 100 * MINUTE;
        let ten = 10 * MINUTE;
        // Thirty long ago and twenty just now: twenty.
        let mut s = started(30, 1, now - ten - MINUTE);
        s.extend(started(20, 1, now - MINUTE));
        assert_eq!(runaway(&s, now, 0), None);
        // A start exactly ten minutes ago is out, one a millisecond sooner is in.
        let mut edge = started(39, 1, now - MINUTE);
        edge.push((MemberId(1), now - ten));
        assert_eq!(runaway(&edge, now, 0), None, "ten minutes ago is out");
        edge.push((MemberId(1), now - ten + 1));
        assert_eq!(runaway(&edge, now, 0).map(|r| r.started), Some(40));
        // Forty just now, but the owner resumed the team after all of them.
        let forty = started(40, 1, now - 2 * MINUTE);
        assert!(runaway(&forty, now, 0).is_some());
        assert_eq!(runaway(&forty, now, now - MINUTE), None);
        // Some before the resume and some after: only those after.
        let mut mixed = started(30, 1, now - 3 * MINUTE);
        mixed.extend(started(15, 1, now - MINUTE / 2));
        assert_eq!(runaway(&mixed, now, now - 2 * MINUTE), None);
        // A start in the future is not one yet.
        assert_eq!(runaway(&started(40, 1, now + 1), now, 0), None);
    }

    #[test]
    fn it_says_who_started_how_many_most_first_and_equal_counts_by_member() {
        let now = 100 * MINUTE;
        let mut s = started(15, 2, now - 1);
        s.extend(started(18, 1, now - 1));
        s.extend(started(7, 3, now - 1));
        let seen = runaway(&s, now, 0).expect("forty");
        assert_eq!(
            seen.by_member,
            [(MemberId(1), 18), (MemberId(2), 15), (MemberId(3), 7)]
        );
        assert_eq!(seen.said(|m| format!("M{}", m.0)), "M1 18, M2 15, M3 7");
        let mut tied = started(20, 5, now - 1);
        tied.extend(started(20, 4, now - 1));
        let seen = runaway(&tied, now, 0).expect("forty");
        assert_eq!(seen.by_member, [(MemberId(4), 20), (MemberId(5), 20)]);
    }
}
