//! Outcomes in the snapshot (ADR 0132): what the team's work came to over the last day and the
//! last week, the turns that wait now, and the owner's way to see at a glance whether activity is
//! buying anything. The measures are pure ([`crate::application::scorecard::outcomes`]); this
//! gathers the rows they read, and reads back the talk against work that the process-health look
//! kept.

use serde_json::{Value, json};

use super::health::TALK_SEEN;
use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::application::scorecard::Window;
use crate::application::scorecard::outcomes::{OutcomeRecord, TalkWork, outcomes, waiting};
use crate::domain::guards::WINDOWS;
use crate::domain::{Event, Millis};

/// Talk against work as the process-health look last kept it: when, over the day and the week.
struct Talk {
    at: Millis,
    day: TalkWork,
    week: TalkWork,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The outcomes for the last day and the last week ending now, the turns that wait now, and
    /// when talk against work was last measured, if it has been. `runs` are the turns that count
    /// as work, `events` the feed without the leads' reports.
    pub(super) fn outcomes_shown(&self, runs: &[Run], events: &[Event]) -> Done<Value> {
        let now = self.clock.now();
        let branches = self.store.branches()?;
        let checks = self.store.checks()?;
        let consults = self.store.consults()?;
        let record = OutcomeRecord {
            runs,
            events,
            branches: &branches,
            checks: &checks,
            consults: &consults,
        };
        let talk = self.talk_kept()?;
        let day = outcomes(&record, &Window::rolling(now, WINDOWS[0].0));
        let week = outcomes(&record, &Window::rolling(now, WINDOWS[1].0));
        Ok(json!({
            "day": day.json(talk.as_ref().map(|t| &t.day)),
            "week": week.json(talk.as_ref().map(|t| &t.week)),
            "waiting": waiting(runs, now).json(),
            "talk_at": talk.map(|t| t.at),
        }))
    }

    /// What the last process-health look measured of talk against work, if it has looked.
    fn talk_kept(&self) -> Done<Option<Talk>> {
        let Some(kept) = self.store.setting(TALK_SEEN)? else {
            return Ok(None);
        };
        let kept: Value = serde_json::from_str(&kept).unwrap_or_default();
        Ok((|| {
            Some(Talk {
                at: kept["at"].as_i64()?,
                day: TalkWork::read(&kept["day"])?,
                week: TalkWork::read(&kept["week"])?,
            })
        })())
    }
}
