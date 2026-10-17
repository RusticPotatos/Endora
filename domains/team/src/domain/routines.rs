//! How the team keeps working (ADR 0107): routines, alarms and check-backs, and the back-off
//! that keeps an empty task from waking its member over and over. Goals are passive; work
//! always arrives as a task; a member never goes looking for work.

use super::work::{GoalId, MemberId, Millis, Refusal, RoutineId, TaskId};

/// A minute.
pub const MINUTE_MS: Millis = 60_000;
/// An hour.
pub const HOUR_MS: Millis = 60 * MINUTE_MS;
/// A day.
pub const DAY_MS: Millis = 24 * HOUR_MS;

/// The shortest rhythm a routine keeps: anything faster would spend the team's day on
/// looking.
pub const SHORTEST_EVERY_MS: Millis = 15 * MINUTE_MS;
/// The longest rhythm a routine keeps.
pub const LONGEST_EVERY_MS: Millis = 30 * DAY_MS;

/// Work that comes round again: a task handed to its member on a rhythm, or when an alarm
/// says something is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    /// Its id.
    pub id: RoutineId,
    /// The title each of its tasks gets.
    pub title: String,
    /// What each of its tasks asks for.
    pub description: String,
    /// The goal its tasks serve.
    pub goal: GoalId,
    /// The member its tasks are handed to.
    pub assignee: MemberId,
    /// How often it fires.
    pub every_ms: Millis,
    /// When it fires next.
    pub next_due_at: Millis,
    /// Whether it is on.
    pub enabled: bool,
    /// Whether it takes the home lab's alarms.
    pub on_lab_trouble: bool,
    /// When it last handed out work or passed an alarm on, if it has.
    pub last_fired_at: Option<Millis>,
    /// The last task it created, if any.
    pub last_task: Option<TaskId>,
}

/// Why a routine fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Firing {
    /// Its time came.
    Schedule,
    /// The owner ran it now.
    Now,
    /// An alarm from Endora, in its words.
    Alarm(String),
}

impl Firing {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Now => "now",
            Self::Alarm(_) => "alarm",
        }
    }

    /// Why it fired, as its task says it.
    #[must_use]
    pub fn why(&self, every_ms: Millis) -> String {
        match self {
            Self::Schedule => format!("It comes round {}.", rhythm(every_ms)),
            Self::Now => "The owner ran it now.".to_owned(),
            Self::Alarm(said) => format!("Endora raised an alarm: {said}."),
        }
    }
}

/// What a firing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireStep {
    /// Hand the assignee a new task.
    NewTask,
    /// Tell the routine's open task what the alarm said.
    PostInOpenTask,
    /// Nothing: its last task is still open.
    Skip,
}

/// What a firing does, given whether the routine's last task is still open. A routine has
/// at most one open task: a firing on the clock while it is open is skipped, and an alarm is
/// passed into it instead.
#[must_use]
pub fn step(firing: &Firing, last_task_open: bool) -> FireStep {
    if !last_task_open {
        return FireStep::NewTask;
    }
    if let Firing::Alarm(_) = firing {
        return FireStep::PostInOpenTask;
    }
    FireStep::Skip
}

/// When a routine fires next after firing at `now`: missed firings are skipped, never caught
/// up.
#[must_use]
pub const fn next_due(now: Millis, every_ms: Millis) -> Millis {
    now + every_ms
}

/// Whether a rhythm is one a routine may keep.
///
/// # Errors
/// [`Refusal::Rhythm`] when it is faster than every 15 minutes or slower than every 30 days.
pub const fn may_keep(every_ms: Millis) -> Result<(), Refusal> {
    if every_ms < SHORTEST_EVERY_MS || every_ms > LONGEST_EVERY_MS {
        return Err(Refusal::Rhythm);
    }
    Ok(())
}

/// A rhythm in words: "every 6 hours", "every day", "every 30 minutes".
#[must_use]
pub fn rhythm(every_ms: Millis) -> String {
    let (n, unit) = if every_ms % DAY_MS == 0 {
        (every_ms / DAY_MS, "day")
    } else if every_ms % HOUR_MS == 0 {
        (every_ms / HOUR_MS, "hour")
    } else {
        ((every_ms / MINUTE_MS).max(1), "minute")
    };
    if n == 1 {
        return format!("every {unit}");
    }
    format!("every {n} {unit}s")
}

// ---- check-backs ------------------------------------------------------------------------

/// The soonest a member may ask to come back, in minutes.
pub const SOONEST_CHECK_BACK: i64 = 5;
/// The latest, in minutes: twelve hours.
pub const LATEST_CHECK_BACK: i64 = 720;
/// The most check-backs a task has waiting.
pub const MOST_CHECK_BACKS: usize = 3;

/// How many minutes a check-back waits: what was asked, held to 5–720.
#[must_use]
pub const fn check_back_minutes(asked: i64) -> i64 {
    if asked < SOONEST_CHECK_BACK {
        return SOONEST_CHECK_BACK;
    }
    if asked > LATEST_CHECK_BACK {
        return LATEST_CHECK_BACK;
    }
    asked
}

/// Where a check-back is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckBackStatus {
    /// Waiting for its time.
    Pending,
    /// Its time came and it woke its member.
    Fired,
    /// A newer one took its place.
    Replaced,
    /// Its task finished first, so it woke nobody.
    Dropped,
}

impl CheckBackStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Fired => "fired",
            Self::Replaced => "replaced",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Pending, Self::Fired, Self::Replaced, Self::Dropped]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// A member's word that it will come back to a task: one wake, later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckBack {
    /// Its id.
    pub id: i64,
    /// Who comes back.
    pub member: MemberId,
    /// To which task.
    pub task: TaskId,
    /// Why, in the member's words.
    pub why: String,
    /// When.
    pub due_at: Millis,
    /// Where it is.
    pub status: CheckBackStatus,
    /// When it was asked for.
    pub set_at: Millis,
    /// When it fired, was replaced or dropped.
    pub done_at: Option<Millis>,
}

/// Which pending check-backs on a task a new one from `member` replaces: the member's own
/// older one, and then the oldest, until the task has room for one more.
#[must_use]
pub fn replaced_by_new(pending: &[CheckBack], member: MemberId) -> Vec<i64> {
    let mut out: Vec<i64> = pending
        .iter()
        .filter(|c| c.member == member)
        .map(|c| c.id)
        .collect();
    let mut left: Vec<&CheckBack> = pending.iter().filter(|c| c.member != member).collect();
    left.sort_by_key(|c| (c.set_at, c.id));
    while left.len() >= MOST_CHECK_BACKS {
        out.push(left.remove(0).id);
    }
    out
}

// ---- back-off -----------------------------------------------------------------------------

/// How many turns in a row on a task may move nothing before its stale wakes are delayed.
pub const EMPTY_TURNS_BEFORE_BACK_OFF: usize = 2;
/// The first delay.
pub const FIRST_BACK_OFF_MS: Millis = 2 * MINUTE_MS;
/// The longest delay.
pub const LONGEST_BACK_OFF_MS: Millis = 30 * MINUTE_MS;

/// How long a wake that brings nothing new waits, after `empty` turns in a row on its task
/// moved nothing forward: nothing at first, then 2 minutes, doubling, at most 30.
#[must_use]
pub fn back_off(empty: usize) -> Millis {
    if empty < EMPTY_TURNS_BEFORE_BACK_OFF {
        return 0;
    }
    let doublings = u32::try_from(empty - EMPTY_TURNS_BEFORE_BACK_OFF)
        .unwrap_or(u32::MAX)
        .min(8);
    (FIRST_BACK_OFF_MS << doublings).min(LONGEST_BACK_OFF_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_routine_hands_out_one_task_at_a_time_and_an_alarm_joins_the_open_one() {
        let alarm = Firing::Alarm("sonarr became unhealthy at 14:02".to_owned());
        for firing in [Firing::Schedule, Firing::Now, alarm.clone()] {
            assert_eq!(step(&firing, false), FireStep::NewTask, "{firing:?}");
        }
        assert_eq!(step(&Firing::Schedule, true), FireStep::Skip);
        assert_eq!(step(&Firing::Now, true), FireStep::Skip);
        assert_eq!(step(&alarm, true), FireStep::PostInOpenTask);
    }

    #[test]
    fn a_late_routine_fires_once_and_counts_on_from_then() {
        assert_eq!(next_due(10 * HOUR_MS, 6 * HOUR_MS), 16 * HOUR_MS);
    }

    #[test]
    fn a_rhythm_reads_as_words_and_stays_within_reason() {
        assert_eq!(rhythm(6 * HOUR_MS), "every 6 hours");
        assert_eq!(rhythm(HOUR_MS), "every hour");
        assert_eq!(rhythm(DAY_MS), "every day");
        assert_eq!(rhythm(2 * DAY_MS), "every 2 days");
        assert_eq!(rhythm(30 * MINUTE_MS), "every 30 minutes");
        assert_eq!(
            Firing::Schedule.why(6 * HOUR_MS),
            "It comes round every 6 hours."
        );
        assert_eq!(
            Firing::Alarm("sonarr became unhealthy at 14:02".to_owned()).why(HOUR_MS),
            "Endora raised an alarm: sonarr became unhealthy at 14:02."
        );
        assert!(may_keep(6 * HOUR_MS).is_ok());
        assert_eq!(may_keep(MINUTE_MS), Err(Refusal::Rhythm));
        assert_eq!(may_keep(31 * DAY_MS), Err(Refusal::Rhythm));
    }

    #[test]
    fn a_check_back_waits_five_minutes_to_twelve_hours() {
        assert_eq!(check_back_minutes(1), 5);
        assert_eq!(check_back_minutes(-3), 5);
        assert_eq!(check_back_minutes(30), 30);
        assert_eq!(check_back_minutes(10_000), 720);
    }

    fn pending(id: i64, member: i64, set_at: Millis) -> CheckBack {
        CheckBack {
            id,
            member: MemberId(member),
            task: TaskId(1),
            why: String::new(),
            due_at: set_at + 10 * MINUTE_MS,
            status: CheckBackStatus::Pending,
            set_at,
            done_at: None,
        }
    }

    #[test]
    fn a_newer_check_back_replaces_the_members_own_and_a_task_holds_three() {
        assert!(replaced_by_new(&[], MemberId(1)).is_empty());
        assert_eq!(replaced_by_new(&[pending(4, 1, 0)], MemberId(1)), [4]);
        let three = [pending(4, 2, 5), pending(5, 3, 1), pending(6, 4, 9)];
        assert_eq!(
            replaced_by_new(&three, MemberId(1)),
            [5],
            "the oldest makes room"
        );
        assert_eq!(replaced_by_new(&three, MemberId(3)), [5], "its own first");
        for s in [
            CheckBackStatus::Pending,
            CheckBackStatus::Fired,
            CheckBackStatus::Replaced,
            CheckBackStatus::Dropped,
        ] {
            assert_eq!(CheckBackStatus::from_name(s.name()), Some(s));
        }
    }

    #[test]
    fn empty_turns_back_off_from_two_minutes_doubling_to_thirty() {
        assert_eq!(back_off(0), 0);
        assert_eq!(back_off(1), 0);
        assert_eq!(back_off(2), 2 * MINUTE_MS);
        assert_eq!(back_off(3), 4 * MINUTE_MS);
        assert_eq!(back_off(4), 8 * MINUTE_MS);
        assert_eq!(back_off(5), 16 * MINUTE_MS);
        assert_eq!(back_off(6), 30 * MINUTE_MS);
        assert_eq!(back_off(60), 30 * MINUTE_MS);
    }
}
