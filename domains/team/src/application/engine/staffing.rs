//! Elastic staffing (ADR 0118): furlough and recall, the daily look for members with nothing to
//! do, and a lead's request for staff. Nobody is ever let go: a furloughed member keeps its
//! record, its name and its notebook, and both directions are the owner's to approve. Every
//! rule is code.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{EventKind, Member, MemberId, Millis, Refusal, Role, lessons, money};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The open tasks a member holds or was handed.
    fn open_work_of(&self, member: MemberId) -> Done<usize> {
        Ok(self
            .store
            .tasks()?
            .iter()
            .filter(|t| {
                !t.status.finished() && (t.owner == Some(member) || t.assignee == Some(member))
            })
            .count())
    }

    /// The owner puts a member on furlough (ADR 0118): never woken, holding no task, left out
    /// of every briefing's team and of the share arithmetic, and keeping its record, its name
    /// and its notebook. The CEO is never furloughed, and a member with open work hands it on
    /// first.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::IsTheCeo`],
    /// [`Refusal::AlreadyFurloughed`], [`Refusal::HoldsWork`]; a store failure.
    pub fn furlough(&self, member: MemberId) -> Done<()> {
        let _held = self.held();
        let found = self.member(member)?;
        self.furlough_held_lock(&found, "owner")
    }

    fn furlough_held_lock(&self, found: &Member, by: &str) -> Done<()> {
        if found.role == Role::Ceo {
            return Err(EngineError::Refused(Refusal::IsTheCeo));
        }
        if found.furloughed() {
            return Err(EngineError::Refused(Refusal::AlreadyFurloughed));
        }
        let count = self.open_work_of(found.id)?;
        if count > 0 {
            return Err(EngineError::Refused(Refusal::HoldsWork {
                name: found.name.clone(),
                count,
            }));
        }
        self.store.furlough_member(found.id, self.clock.now())?;
        self.emit(
            EventKind::MemberFurloughed,
            json!({ "member": found.id.0, "name": found.name, "by": by }),
        )
    }

    /// The owner brings a furloughed member back to work (ADR 0118).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::NotFurloughed`];
    /// a store failure.
    pub fn recall(&self, member: MemberId) -> Done<()> {
        let _held = self.held();
        let found = self.member(member)?;
        self.recall_held_lock(&found, None, "owner")
    }

    fn recall_held_lock(&self, found: &Member, reports_to: Option<MemberId>, by: &str) -> Done<()> {
        if !found.furloughed() {
            return Err(EngineError::Refused(Refusal::NotFurloughed));
        }
        self.store.recall_member(found.id, self.clock.now())?;
        if let Some(to) = reports_to {
            self.store.set_reports_to(found.id, Some(to))?;
        }
        self.emit(
            EventKind::MemberRecalled,
            json!({ "member": found.id.0, "name": found.name, "by": by,
                    "reports_to": reports_to.map(|m| m.0) }),
        )
    }

    /// Today in the owner's time with its weekday and year, as `Sat 18 Oct 2026`.
    pub(super) fn said_today(&self) -> String {
        let day = money::day_of(self.clock.now(), self.settings.offset_minutes);
        let (year, _, _) = lessons::civil(day);
        let (d, month) = lessons::day_and_month(day);
        // The epoch day 0 was a Thursday.
        let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][day.rem_euclid(7) as usize];
        format!("{weekday} {d} {month} {year}")
    }

    /// The day and month of `at` in the owner's time, as `16 Oct`.
    pub(super) fn said_day(&self, at: Millis) -> String {
        let (day, month) = lessons::day_and_month(money::day_of(at, self.settings.offset_minutes));
        format!("{day} {month}")
    }
}
