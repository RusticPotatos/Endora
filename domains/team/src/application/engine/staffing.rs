//! Elastic staffing (ADR 0118): furlough and recall, the daily look for members with nothing to
//! do, and a lead's request for staff. Nobody is ever let go: a furloughed member keeps its
//! record, its name and its notebook, and both directions are the owner's to approve. Every
//! rule is code.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, TeamStore, WorkerAdapter};
use crate::domain::staffing::{self, RecallRequest};
use crate::domain::{
    DeskId, EventKind, Member, MemberId, Millis, ProjectId, Ref, Refusal, Role, RunId, WakeReason,
    lessons, money,
};

/// The owner's day the idle look last ran on, so it runs once a day.
const IDLE_CHECKED_DAY: &str = "idle_checked_day";

/// When the team first saw a member, which is as far back as its idleness is counted for a
/// member the store has no join date for.
fn since_key(member: MemberId) -> String {
    format!("member_since:{}", member.0)
}

/// When the owner last kept a member the idle look offered for furlough.
fn kept_key(member: MemberId) -> String {
    format!("idle_kept:{}", member.0)
}

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

    /// The look at the team, once a day, on the day's first tick (ADR 0118): a member that is
    /// not the CEO or a lead, is not on furlough, holds no task, has no routine of its own on,
    /// and has been woken, handed something and taken a turn in none of the last seven days, is
    /// offered to the owner for furlough. One item a member; not again for a week after the
    /// owner keeps it. Returns how many were offered.
    pub(super) fn idle_look(&self) -> Done<usize> {
        let day = self.today();
        if self.store.setting(IDLE_CHECKED_DAY)?.as_deref() == Some(day.to_string().as_str()) {
            return Ok(0);
        }
        self.store.set_setting(IDLE_CHECKED_DAY, &day.to_string())?;
        let now = self.clock.now();
        let tasks = self.store.tasks()?;
        // Downtime is nothing to do, so it is not what keeps a member from being offered for
        // furlough (ADR 0119): its wakes and turns are left out.
        let downtime = self.downtime_wakes()?;
        let runs: Vec<_> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| !downtime.contains(&r.wake))
            .collect();
        let wakes: Vec<_> = self
            .store
            .wakes()?
            .into_iter()
            .filter(|w| w.reason != WakeReason::Downtime)
            .collect();
        let routines = self.store.routines()?;
        let desk = self.store.desk()?;
        let mut offered = 0;
        for member in self
            .store
            .members()?
            .iter()
            .filter(|m| staffing::may_be_offered(m))
        {
            // A member the team has not met yet is met now; a recalled one starts again.
            let met = match self.store.setting(&since_key(member.id))? {
                Some(said) => said.parse().unwrap_or(now),
                None => {
                    self.store
                        .set_setting(&since_key(member.id), &now.to_string())?;
                    now
                }
            };
            let baseline = met.max(member.recalled_at.unwrap_or(Millis::MIN));
            let holds = tasks.iter().any(|t| {
                !t.status.finished()
                    && (t.owner == Some(member.id) || t.assignee == Some(member.id))
            });
            let has_routine = routines
                .iter()
                .any(|r| r.enabled && r.assignee == member.id);
            if holds || has_routine {
                continue;
            }
            let last = runs
                .iter()
                .filter(|r| r.member == member.id)
                .map(|r| r.queued_at)
                .chain(wakes.iter().filter(|w| w.member == member.id).map(|w| w.at))
                .max();
            let Some(since) = staffing::idle_since(baseline, last, now) else {
                continue;
            };
            let kept = self
                .store
                .setting(&kept_key(member.id))?
                .and_then(|said| said.parse::<Millis>().ok());
            if kept.is_some_and(|at| now - at < staffing::KEPT_FOR_MS) {
                continue;
            }
            if desk.iter().any(|d| {
                d.kind == "furlough" && d.status == "waiting" && d.member == Some(member.id)
            }) {
                continue;
            }
            let text = staffing::offer_text(&member.name, &self.said_day(since));
            let item = self
                .store
                .add_desk("furlough", None, None, Some(member.id), &text, now)?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": item.0, "kind": "furlough", "member": member.id.0,
                        "name": member.name }),
            )?;
            offered += 1;
        }
        Ok(offered)
    }

    /// The owner's word on an offer of furlough (the lock held): approved, the member goes on
    /// furlough; declined, it is kept, and not offered again for a week.
    pub(super) fn decide_furlough(&self, found: &DeskItem, approve: bool) -> Done<()> {
        let member = self.member(found.member.ok_or(EngineError::Unknown("member"))?)?;
        if approve {
            return self.furlough_held_lock(&member, "owner");
        }
        self.store
            .set_setting(&kept_key(member.id), &self.clock.now().to_string())?;
        Ok(())
    }

    /// A lead asks the owner for staff for a project that has a backlog (ADR 0118): why, and
    /// the project's open tasks as evidence, at least twice the project's active developers,
    /// each checked to exist, be open and be in the project. A furloughed developer is offered
    /// back first (a recall on the desk); with none, it becomes the hire request (ADR 0115),
    /// under the same seats. Approving brings the member back, or hires, reporting to the lead.
    ///
    /// # Errors
    /// [`Refusal::OnlyLeadsStaff`], [`Refusal::NotYourProject`], [`Refusal::NoEvidence`],
    /// [`Refusal::NoSuchEvidence`], [`Refusal::NotOpenInProject`], [`Refusal::NoBacklog`],
    /// [`Refusal::AlreadyAsked`], and what a hire refuses (the wing's seats).
    pub fn request_staff(
        &self,
        run: RunId,
        project: ProjectId,
        why: &str,
        cited: &[Ref],
    ) -> Done<DeskId> {
        let _held = self.held();
        let turn = self.running(run)?;
        let asker = self.member(turn.member)?;
        if asker.role != Role::Lead {
            return Err(EngineError::Refused(Refusal::OnlyLeadsStaff));
        }
        let found = self.project(project)?;
        if found.lead != asker.id {
            return Err(EngineError::Refused(Refusal::NotYourProject));
        }
        if cited.is_empty() {
            return Err(EngineError::Refused(Refusal::NoEvidence));
        }
        let tasks = self.store.tasks()?;
        let mut open = Vec::new();
        for r in cited {
            let Ref::Task(id) = r else {
                return Err(EngineError::Refused(Refusal::NotOpenInProject(r.said())));
            };
            let Some(task) = tasks.iter().find(|t| t.id == *id) else {
                return Err(EngineError::Refused(Refusal::NoSuchEvidence(r.said())));
            };
            if task.status.finished() || task.project != Some(project) {
                return Err(EngineError::Refused(Refusal::NotOpenInProject(r.said())));
            }
            if !open.contains(id) {
                open.push(*id);
            }
        }
        let members = self.store.members()?;
        let developers = members
            .iter()
            .filter(|m| m.role == staffing::STAFF_ROLE && m.at_work())
            .filter(|m| {
                m.reports_to == Some(found.lead)
                    || tasks.iter().any(|t| {
                        !t.status.finished()
                            && t.project == Some(project)
                            && (t.owner == Some(m.id) || t.assignee == Some(m.id))
                    })
            })
            .count();
        staffing::may_ask(open.len(), developers).map_err(EngineError::Refused)?;
        let Some(back) = staffing::to_recall(&members, staffing::STAFF_ROLE, asker.id) else {
            return self.hire_asked(
                &turn,
                &asker,
                None,
                staffing::STAFF_ROLE,
                crate::domain::Brain::Haiku.name(),
                why,
                cited,
            );
        };
        if self.store.desk()?.iter().any(|d| {
            matches!(d.kind.as_str(), "hire" | "recall")
                && d.status == "waiting"
                && d.member == Some(asker.id)
        }) {
            return Err(EngineError::Refused(Refusal::AlreadyAsked));
        }
        let returning = self.member(back)?;
        let request = RecallRequest {
            member: back,
            reports_to: asker.id,
            why: super::one_line(why, 400),
        };
        let text = request.write(&returning.name, &asker.name, &self.evidence_said(cited)?);
        let item = self.store.add_desk(
            "recall",
            turn.task,
            None,
            Some(asker.id),
            &text,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "recall", "task": turn.task.map(|t| t.0),
                    "member": asker.id.0, "name": returning.name }),
        )?;
        Ok(item)
    }

    /// The owner's word on a recall (the lock held): approved, the member is back at work and
    /// reports to the lead who asked; either way the lead hears in the task's thread.
    pub(super) fn decide_recall(&self, found: &DeskItem, approve: bool) -> Done<()> {
        let request =
            RecallRequest::read(&found.text).ok_or(EngineError::Unknown("recall request"))?;
        let member = self.member(request.member)?;
        let said = match (approve, member.furloughed()) {
            (false, _) => format!("The owner declined bringing {} back.", member.name),
            (true, false) => format!("{} is already back at work.", member.name),
            (true, true) => {
                self.recall_held_lock(&member, Some(request.reports_to), "owner, asked by a lead")?;
                format!(
                    "The owner approved: {} is back from furlough, reporting to you.",
                    member.name
                )
            }
        };
        if let Some(task) = found.task {
            self.post_held_lock(Author::Owner, task, &said)?;
            if let Some(asker) = found.member {
                self.wake_held_lock(asker, Some(task), WakeReason::ProposalDecided)?;
            }
        }
        Ok(())
    }

    /// Each lead's team for the scorecard: seats, active, furloughed.
    pub(super) fn teams_shown(&self) -> Done<serde_json::Value> {
        let members = self.store.members()?;
        Ok(json!(
            staffing::teams(&members)
                .iter()
                .map(|t| json!({
                    "lead": t.lead.0,
                    "name": members.iter().find(|m| m.id == t.lead).map(|m| m.name.clone()),
                    "seats": t.seats, "active": t.active, "furloughed": t.furloughed,
                }))
                .collect::<Vec<_>>()
        ))
    }
}
