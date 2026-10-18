//! Roles as an enforced RACI (ADR 0120): the owner's product-model org of 2026-10-18. The CEO
//! sets direction and owns the outcome; the lead of an area (a lead and those who report to
//! it, owning the area's projects) runs it, decides what is done in it, and reports up. Agents
//! drift across such lines, so the code holds them: on a task in a lead's area the CEO reads
//! and talks, and gives the lead a direction, but never claims, moves, closes, accepts or sends
//! back its work, and is never woken by its alarms.
//!
//! An area is derived from the record, never from a name: a task's area is its project's,
//! when a lead leads the project; otherwise the lead its holder reports up to; otherwise the
//! earliest lead at work. A task in a project the CEO leads, or held by the CEO in no project
//! (its own reviews and planning), is the CEO's own.

use super::{Author, Done, Engine, EngineError, one_line};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::chain;
use crate::domain::{Member, MemberId, Ref, Refusal, Role, RunId, Task, TaskId, WakeReason};

/// The most characters a direction may run to.
pub const DIRECTION_MOST: usize = 1_000;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The lead whose area `task` is in, at work; `None` for the CEO's own work, and for a
    /// team with no lead at work (then the CEO works as it did before, ADR 0111).
    pub(super) fn area_lead(&self, task: &Task) -> Done<Option<Member>> {
        let members = self.store.members()?;
        let lead_at_work = |id: MemberId| {
            members
                .iter()
                .find(|m| m.id == id && m.role == Role::Lead && m.at_work())
                .cloned()
        };
        if let Some(project) = task.project {
            let lead = self.project(project)?.lead;
            if members.iter().any(|m| m.id == lead && m.role == Role::Ceo) {
                return Ok(None);
            }
            if let Some(found) = lead_at_work(lead) {
                return Ok(Some(found));
            }
        }
        let holder = task
            .owner
            .or(task.assignee)
            .and_then(|h| members.iter().find(|m| m.id == h));
        if let Some(holder) = holder {
            if holder.role == Role::Ceo && task.project.is_none() {
                return Ok(None);
            }
            let mut seen = Vec::new();
            let mut at = Some(holder);
            while let Some(member) = at {
                if member.role == Role::Lead && member.at_work() {
                    return Ok(Some(member.clone()));
                }
                if seen.contains(&member.id) {
                    break;
                }
                seen.push(member.id);
                at = chain::manager(member, &members)
                    .and_then(|id| members.iter().find(|m| m.id == id));
            }
        }
        Ok(members
            .iter()
            .filter(|m| m.role == Role::Lead && m.at_work())
            .min_by_key(|m| m.id)
            .cloned())
    }

    /// Refuses the CEO's hand on `task`'s work when the task is in a lead's area: the lead
    /// decides there, and the CEO directs it ([`Refusal::CeoDirects`]). Anyone else, and the
    /// CEO on its own work, passes.
    pub(super) fn ceo_keeps_out(&self, member: MemberId, task: &Task) -> Done<()> {
        if self.member(member)?.role != Role::Ceo {
            return Ok(());
        }
        let Some(lead) = self.area_lead(task)? else {
            return Ok(());
        };
        Err(EngineError::Refused(Refusal::CeoDirects(lead.name)))
    }

    /// Whom an alarm or a recovery about `task` wakes: whoever has it, unless that is the CEO,
    /// who is never woken by the lab's alarms (ADR 0120); then the lead of its area, who is
    /// handed the task. `fallback` when nobody has it.
    pub(super) fn alarm_to(&self, task: &Task, fallback: MemberId) -> Done<MemberId> {
        let holder = task.owner.or(task.assignee).unwrap_or(fallback);
        if self.member(holder)?.role != Role::Ceo {
            return Ok(holder);
        }
        let Some(lead) = self.area_lead(task)? else {
            return Ok(holder);
        };
        if task.owner.or(task.assignee) == Some(holder) {
            self.store.release(task.id)?;
            self.store.set_assignee(task.id, Some(lead.id))?;
            self.post_held_lock(
                Author::Endora,
                task.id,
                &format!(
                    "This is {}'s area: it goes to {}, who runs it. The CEO is told, never woken, \
                     by the lab's alarms.",
                    lead.name, lead.name
                ),
            )?;
        }
        Ok(lead.id)
    }

    /// The CEO's turn gives the lead of `task`'s area a direction (ADR 0120): what it wants
    /// done or looked at, on evidence from HQ's record, posted in the task's thread as
    /// "Direction for Ada: …", waking the lead. The lead decides: it acts, hands it on in its
    /// area, or answers why not. Returns the lead.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`]; [`Refusal::Finished`];
    /// [`EngineError::Unknown`] for no direction; what the evidence rule refuses;
    /// [`Refusal::NoAreaLead`] for a task in no lead's area.
    pub fn direct_in(
        &self,
        run: RunId,
        task: TaskId,
        direction: &str,
        cited: &[Ref],
    ) -> Done<MemberId> {
        let _held = self.held();
        let (turn, ceo) = self.the_ceos_turn(run)?;
        let target = self.task(task)?;
        if target.status.finished() {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        let direction = one_line(direction, DIRECTION_MOST);
        if direction.is_empty() {
            return Err(EngineError::Unknown("direction"));
        }
        self.judge_decision(cited, turn.id)?;
        let lead = self
            .area_lead(&target)?
            .ok_or(EngineError::Refused(Refusal::NoAreaLead))?;
        let said = self.evidence_said(cited)?;
        self.post_held_lock(
            Author::Member(ceo.id),
            task,
            &chain::direction(&lead.name, &direction, &said),
        )?;
        // A direction is a decision of the CEO's: its turn moved something.
        if let Some(own) = turn.task {
            self.mark_progress(ceo.id, own)?;
        }
        self.mark_progress(ceo.id, task)?;
        self.wake_held_lock(lead.id, Some(task), WakeReason::Directed)?;
        Ok(lead.id)
    }

    /// What the lead is told on a turn the CEO's direction woke: the direction, and that the
    /// lead decides.
    pub(super) fn direction_said(&self, lead: &Member, task: TaskId) -> Done<String> {
        let thread = self.store.thread(task)?;
        let posts: Vec<(&str, &str)> = thread
            .iter()
            .map(|p| (p.author.as_str(), p.text.as_str()))
            .collect();
        let Some((by, said)) = chain::latest_direction(&posts, &lead.name) else {
            return Ok(String::new());
        };
        Ok(format!(
            "{by}, the CEO, directs you: {said}\nThis is your area, and you decide: act on it, \
             hand it on in your team with create_task, or answer {by} in the thread (post) with \
             why not, on evidence.\n"
        ))
    }
}
