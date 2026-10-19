//! Caps on a runaway (ADR 0133), in the engine: a member's day of turns on the house's own
//! models, which cost nothing and so are bounded by nothing else, and the tripwire that stops the
//! team itself when it starts more turns in ten minutes than any real work does.
//!
//! The scripted demo ran 1,539 turns in 3 h 40 min, and until now only the owner could pull the
//! kill switch: the plan's tripwires were never built. Both caps are on from the start, because a
//! runaway is what they are for. The first loses nothing (a turn over its member's day waits for
//! the day to turn, like a turn waiting for budget or for the Mac); the second stops the team the
//! way the owner's switch does, with one card on the desk saying who started how many, and the
//! owner resumes it as ever. A scripted team (the demo) is exempt from both.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::caps::{self, Runaway};
use crate::domain::guards::THRESHOLDS;
use crate::domain::money;
use crate::domain::{Brain, EventKind, MemberId, Millis};

/// Where HQ keeps when the owner last resumed the team: the turns it started before are the
/// owner's to have looked at, and do not count against what comes next.
const TRIPWIRE_SINCE: &str = "tripwire_since";
/// Where HQ keeps who stopped the team, while it is stopped: `owner` or `tripwire`.
pub(super) const STOPPED_BY: &str = "stopped_by";

/// Why the tripwire says it stopped the team.
pub(super) const TRIPPED: &str = "stopped by the runaway tripwire";

/// What the caps read of the turns started, once for a tick and kept up as turns start in it.
pub(super) struct Starts {
    /// Every turn started in the tripwire's window: who, and when.
    recent: Vec<(MemberId, Millis)>,
    /// How many turns each member has started today on the house's own models.
    free_today: HashMap<MemberId, usize>,
    /// Today's first moment, and the first moment after it, by the owner's day.
    today: (Millis, Millis),
    /// When the owner last resumed the team.
    since: Millis,
    /// The team is scripted (the demo): neither cap applies to it.
    exempt: bool,
}

impl Starts {
    /// How many turns `run`'s member has started today on the house's own models, the turn being
    /// placed aside: one that started earlier today and is placed again (a retry) is counted
    /// once, not against itself.
    pub(super) fn free_turns_of(&self, run: &Run) -> usize {
        if self.exempt {
            return 0;
        }
        let taken = self.free_today.get(&run.member).copied().unwrap_or(0);
        let counted = run
            .started_at
            .is_some_and(|at| self.today.0 <= at && at < self.today.1)
            && run.brain.is_some_and(Brain::house);
        taken - usize::from(counted)
    }

    /// The runaway the team has come to, if starting one more turn would make one.
    pub(super) fn runaway(&self, now: Millis) -> Option<Runaway> {
        if self.exempt {
            return None;
        }
        caps::runaway(&self.recent, now, self.since)
    }

    /// A turn of `member`'s just started on `brain`.
    pub(super) fn started(&mut self, member: MemberId, brain: Brain, now: Millis) {
        self.recent.push((member, now));
        if brain.house() {
            *self.free_today.entry(member).or_insert(0) += 1;
        }
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// What the caps read of the turns started so far, as of `now`.
    pub(super) fn starts_read(&self, now: Millis) -> Done<Starts> {
        let day = self.today();
        let offset = self.settings.offset_minutes;
        let today = (
            money::day_start(day, offset),
            money::day_start(day + 1, offset),
        );
        let since = self
            .store
            .setting(TRIPWIRE_SINCE)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(Millis::MIN);
        let mut starts = Starts {
            recent: Vec::new(),
            free_today: HashMap::new(),
            today,
            since,
            exempt: self.settings.scripted,
        };
        for run in self.store.runs()? {
            let Some(at) = run.started_at else {
                continue;
            };
            if now - at < THRESHOLDS.runaway_window_ms {
                starts.recent.push((run.member, at));
            }
            if today.0 <= at && at < today.1 && run.brain.is_some_and(Brain::house) {
                *starts.free_today.entry(run.member).or_insert(0) += 1;
            }
        }
        Ok(starts)
    }

    /// The team started as many turns as it may in the tripwire's window and another was about to
    /// start: HQ stops the team itself, the way the owner's switch does, and leaves one card on
    /// the owner's desk saying who started how many (ADR 0133). The owner resumes it as ever.
    pub(super) fn trip(&self, ran: &Runaway) -> Done<()> {
        let members = self.store.members()?;
        let name = |id: MemberId| {
            members
                .iter()
                .find(|m| m.id == id)
                .map_or_else(|| "someone".to_owned(), |m| m.name.clone())
        };
        let window = THRESHOLDS.runaway_window_ms;
        let per: Vec<Value> = ran
            .by_member
            .iter()
            .map(|(m, turns)| json!({ "member": m.0, "name": name(*m), "turns": turns }))
            .collect();
        self.stop_all_held_lock(
            "tripwire",
            TRIPPED,
            json!({ "turns": ran.started, "window_ms": window, "members": per }),
        )?;
        // One card, however often it trips before the owner has looked.
        if self
            .store
            .desk()?
            .iter()
            .any(|d| d.kind == "tripwire" && d.status == "waiting")
        {
            return Ok(());
        }
        let text = format!(
            "Stopped by the runaway tripwire: the team started {} turns in {} minutes, and \
             another was about to start; more than {} is a runaway. Turns started, by member: \
             {}. Every turn was stopped, and nothing starts until you resume. The activity log \
             shows what they were doing.",
            ran.started,
            window / 60_000,
            THRESHOLDS.runaway_turns,
            ran.said(name)
        );
        let desk = self
            .store
            .add_desk("tripwire", None, None, None, &text, self.clock.now())?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "tripwire" }),
        )
    }

    /// The owner resumes the team (the lock held): what it started before is not held against
    /// what it starts next, and the tripwire's card, if it was the wire that stopped it, is
    /// answered.
    pub(super) fn resume_held_lock(&self) -> Done<()> {
        self.store.set_stopped(false)?;
        self.store
            .set_setting(TRIPWIRE_SINCE, &self.clock.now().to_string())?;
        self.store.set_setting(STOPPED_BY, "")?;
        for item in self.store.desk()? {
            if item.kind == "tripwire" && item.status == "waiting" {
                self.settle_desk_item(item.id, &item.kind, "resumed")?;
            }
        }
        self.emit(EventKind::Resumed, json!({ "by": "owner" }))
    }
}
