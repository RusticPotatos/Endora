//! When a container may be restarted: once an hour each, ten a day in all, and never a third
//! time in a day.

use super::{DAY_MS, HOUR_MS};

/// How many restarts a day, across every container.
pub const MOST_RESTARTS_A_DAY: usize = 10;
/// The restarts of one container in a day after which it "keeps dying".
pub const KEEPS_DYING_AFTER: usize = 2;

/// Whether a restart may go ahead, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartVerdict {
    /// Go ahead.
    Restart,
    /// It was restarted less than an hour ago and stopped again: the restart did not hold.
    DidNotHold,
    /// It has been restarted twice today already.
    KeepsDying,
    /// Ten restarts today already, across every container.
    DailyLimit,
}

impl RestartVerdict {
    /// Why it was not restarted, as the owner reads it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Restart => "",
            Self::DidNotHold => "It stopped again within an hour of a restart, so I left it",
            Self::KeepsDying => "It keeps dying: restarted twice today already, so I left it",
            Self::DailyLimit => "I've restarted ten containers today, my limit, so I left it",
        }
    }
}

/// Whether this container may be restarted now, given when it and every container were
/// restarted before.
#[must_use]
pub fn may_restart(this_one: &[i64], all: &[i64], now: i64) -> RestartVerdict {
    let today = |at: &&i64| now - **at < DAY_MS;
    if this_one.iter().any(|at| now - *at < HOUR_MS) {
        return RestartVerdict::DidNotHold;
    }
    if this_one.iter().filter(today).count() >= KEEPS_DYING_AFTER {
        return RestartVerdict::KeepsDying;
    }
    if all.iter().filter(today).count() >= MOST_RESTARTS_A_DAY {
        return RestartVerdict::DailyLimit;
    }
    RestartVerdict::Restart
}
