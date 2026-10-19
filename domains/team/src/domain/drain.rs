//! A drain before a restart (ADR 0134): while one lasts nothing new starts, the turns under way
//! are told to wrap up, and what the restart cut is counted. Deploys, and the NAS's own
//! security reboots, cut turns of ten or fifteen minutes in the middle of a step; the work was
//! lost with them. The rules here are pure: the engine keeps the drain and acts on it.

use super::work::{Millis, RunId};

/// How long a deploy waits for the turns under way to end, unless it says otherwise.
pub const DEFAULT_CAP_SECONDS: i64 = 300;
/// The longest a deploy may ask to wait: a turn is stopped after half an hour anyway.
pub const LONGEST_CAP_SECONDS: i64 = 1_800;
/// A drain lifts itself after this long, so one nobody ended never stalls the team.
pub const MAX_AGE_MS: Millis = 30 * 60_000;

/// What every answer HQ gives a running turn ends with while a drain lasts.
pub const NOTICE: &str =
    "A restart is coming: finish this step, leave a handover note, and end your turn.";

/// How to leave the note, said after [`NOTICE`]: with the tool a member already posts with.
pub const HOW_TO_LEAVE_IT: &str = "Leave it with post: give `handover` (done, not done, next, \
    watch out) as well as your text, and your turn ends. The task stays yours, and the note is \
    the first thing you read when it goes on after the restart.";

/// How a drain ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifted {
    /// Somebody said resume.
    Resumed,
    /// HQ started again: the restart the drain was for.
    Restarted,
    /// It lasted [`MAX_AGE_MS`] and nobody ended it.
    Forgotten,
}

/// A drain under way: when it began, how long it may wait for turns, and which turns were
/// running when it began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drain {
    /// When it began.
    pub since: Millis,
    /// How long, in seconds, a deploy waits for the turns under way.
    pub cap_seconds: i64,
    /// The turns that were running when it began.
    pub holding: Vec<RunId>,
}

impl Drain {
    /// A drain beginning `now`, with the cap asked for (the default when none, never beyond
    /// [`LONGEST_CAP_SECONDS`]) and the turns running.
    #[must_use]
    pub fn begin(now: Millis, cap_seconds: Option<i64>, holding: Vec<RunId>) -> Self {
        Self {
            since: now,
            cap_seconds: cap_seconds
                .unwrap_or(DEFAULT_CAP_SECONDS)
                .clamp(1, LONGEST_CAP_SECONDS),
            holding,
        }
    }

    /// Whether it has lasted [`MAX_AGE_MS`] by `now`.
    #[must_use]
    pub fn forgotten(&self, now: Millis) -> bool {
        now - self.since >= MAX_AGE_MS
    }

    /// The drain as the store keeps it.
    #[must_use]
    pub fn keep(&self) -> String {
        let holding: Vec<String> = self.holding.iter().map(|r| r.0.to_string()).collect();
        format!("{}|{}|{}", self.since, self.cap_seconds, holding.join(","))
    }

    /// A drain from what the store kept; `None` for nothing, or for what cannot be read.
    #[must_use]
    pub fn read(kept: &str) -> Option<Self> {
        let mut parts = kept.split('|');
        let since = parts.next()?.parse().ok()?;
        let cap_seconds = parts.next()?.parse().ok()?;
        let holding = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        let holding = holding
            .split(',')
            .filter(|id| !id.is_empty())
            .map(|id| id.parse().map(RunId))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        Some(Self {
            since,
            cap_seconds,
            holding,
        })
    }
}

/// What a drain came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// When the drain began.
    pub since: Millis,
    /// How it ended.
    pub by: Lifted,
    /// How many turns were running when it began.
    pub held: usize,
    /// How many of them ended before it did.
    pub clean: usize,
    /// How many the restart cut.
    pub cut: usize,
}

impl Outcome {
    /// What `drain` came to when it was lifted `by`, with `running` the turns still running.
    /// Only a restart cuts a turn: any that is still running when HQ starts again is lost.
    #[must_use]
    pub fn of(drain: &Drain, running: &[RunId], by: Lifted) -> Self {
        let still = drain.holding.iter().filter(|r| running.contains(r)).count();
        Self {
            since: drain.since,
            by,
            held: drain.holding.len(),
            clean: drain.holding.len() - still,
            cut: if by == Lifted::Restarted {
                running.len()
            } else {
                0
            },
        }
    }

    /// The outcome as the store keeps it.
    #[must_use]
    pub fn keep(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.since,
            self.by.name(),
            self.held,
            self.clean,
            self.cut
        )
    }

    /// An outcome from what the store kept.
    #[must_use]
    pub fn read(kept: &str) -> Option<Self> {
        let parts: Vec<&str> = kept.split('|').collect();
        let [since, by, held, clean, cut] = parts.as_slice() else {
            return None;
        };
        Some(Self {
            since: since.parse().ok()?,
            by: Lifted::from_name(by)?,
            held: held.parse().ok()?,
            clean: clean.parse().ok()?,
            cut: cut.parse().ok()?,
        })
    }
}

impl Lifted {
    /// Its name, in the feed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Resumed => "resume",
            Self::Restarted => "restart",
            Self::Forgotten => "max age",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Resumed, Self::Restarted, Self::Forgotten]
            .into_iter()
            .find(|l| l.name() == name)
    }
}

/// What the owner is told when a restart cut turns: how many, and which, as `who` says them.
#[must_use]
pub fn cut_said(who: &[String]) -> String {
    let turns = if who.len() == 1 { "turn" } else { "turns" };
    format!(
        "A restart cut {} {turns}: {}. Each is tried again, from its start; what it had done \
         since its last note is lost.",
        who.len(),
        who.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ids: &[i64]) -> Vec<RunId> {
        ids.iter().copied().map(RunId).collect()
    }

    #[test]
    fn a_drain_waits_five_minutes_unless_it_is_told_otherwise_and_never_beyond_half_an_hour() {
        let day = 1_000;
        assert_eq!(Drain::begin(day, None, ids(&[])).cap_seconds, 300);
        assert_eq!(Drain::begin(day, Some(90), ids(&[])).cap_seconds, 90);
        assert_eq!(Drain::begin(day, Some(0), ids(&[])).cap_seconds, 1);
        assert_eq!(Drain::begin(day, Some(-5), ids(&[])).cap_seconds, 1);
        assert_eq!(Drain::begin(day, Some(86_400), ids(&[])).cap_seconds, 1_800);
    }

    #[test]
    fn a_drain_is_forgotten_after_thirty_minutes_and_not_before() {
        let drain = Drain::begin(10_000, None, ids(&[1]));
        assert!(!drain.forgotten(10_000));
        assert!(!drain.forgotten(10_000 + MAX_AGE_MS - 1));
        assert!(drain.forgotten(10_000 + MAX_AGE_MS));
    }

    #[test]
    fn a_drain_is_kept_and_read_back_whole() {
        let drain = Drain::begin(1_792_000_000_000, Some(120), ids(&[4, 9]));
        assert_eq!(Drain::read(&drain.keep()), Some(drain));
        let none = Drain::begin(5, None, ids(&[]));
        assert_eq!(Drain::read(&none.keep()), Some(none));
    }

    #[test]
    fn nothing_kept_or_a_muddle_is_no_drain() {
        assert_eq!(Drain::read(""), None);
        assert_eq!(Drain::read("not a drain"), None);
        assert_eq!(Drain::read("12|x|1,2"), None);
    }

    #[test]
    fn only_a_restart_cuts_a_turn_and_the_rest_ended_before_it() {
        let drain = Drain::begin(100, None, ids(&[1, 2, 3]));
        let restarted = Outcome::of(&drain, &ids(&[3]), Lifted::Restarted);
        assert_eq!((restarted.held, restarted.clean, restarted.cut), (3, 2, 1));
        let resumed = Outcome::of(&drain, &ids(&[3]), Lifted::Resumed);
        assert_eq!((resumed.held, resumed.clean, resumed.cut), (3, 2, 0));
        let forgotten = Outcome::of(&drain, &ids(&[]), Lifted::Forgotten);
        assert_eq!((forgotten.held, forgotten.clean, forgotten.cut), (3, 3, 0));
        assert_eq!(restarted.since, 100);
        assert_eq!(restarted.by, Lifted::Restarted);
    }

    #[test]
    fn an_outcome_is_kept_and_read_back_whole() {
        let drain = Drain::begin(100, None, ids(&[1, 2]));
        for by in [Lifted::Resumed, Lifted::Restarted, Lifted::Forgotten] {
            let outcome = Outcome::of(&drain, &ids(&[2]), by);
            assert_eq!(Outcome::read(&outcome.keep()), Some(outcome));
        }
        assert_eq!(Outcome::read(""), None);
    }

    #[test]
    fn what_a_restart_cut_is_said_in_the_owners_words() {
        let one = cut_said(&["Zed on #12 Cap sonarr's memory".to_owned()]);
        assert!(one.starts_with("A restart cut 1 turn: Zed on #12"), "{one}");
        let two = cut_said(&["Zed on #12 A".to_owned(), "Ada on #3 B".to_owned()]);
        assert!(
            two.starts_with("A restart cut 2 turns: Zed on #12 A; Ada on #3 B"),
            "{two}"
        );
        assert!(two.contains("tried again"), "{two}");
    }

    #[test]
    fn the_notice_is_the_owners_words() {
        assert_eq!(
            NOTICE,
            "A restart is coming: finish this step, leave a handover note, and end your turn."
        );
    }

    #[test]
    fn a_lift_has_a_name_for_the_feed() {
        assert_eq!(Lifted::Resumed.name(), "resume");
        assert_eq!(Lifted::Restarted.name(), "restart");
        assert_eq!(Lifted::Forgotten.name(), "max age");
    }
}
