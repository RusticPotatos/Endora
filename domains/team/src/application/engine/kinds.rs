//! The kinds of turn, in the engine (ADR 0131): what a turn needs, read from why it was asked
//! for and never from what a member says, and the notice on the owner's desk when an alarm was
//! handled on a house model for want of budget, the one judgment that may be.

use std::collections::HashSet;

use serde_json::json;

use super::Done;
use super::Engine;
use crate::application::ports::{Clock, Run, TeamStore, Wake, WorkerAdapter};
use crate::domain::cut::cut;
use crate::domain::turns::Need;
use crate::domain::{Brain, EventKind, Member, MemberId, Ref, Role, TaskId, WakeReason};

/// What deciding the kind of a queued turn reads besides its wakes, read once a tick.
pub(super) struct Known {
    /// The tasks that are a lead's standing review (its report on its area, its patterns).
    reports: HashSet<TaskId>,
    /// Who sits in the engineering wing.
    wing: Vec<MemberId>,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// What deciding the kinds of this tick's queued turns reads.
    pub(super) fn known_for_kinds(&self) -> Done<Known> {
        Ok(Known {
            reports: self.report_task_ids()?,
            wing: self.wing_members()?,
        })
    }

    /// What a turn asked for by `reason`, about `task`, for `member`, needs: decided here from
    /// why and what it is about, and never by the member.
    pub(super) fn need_of_wake(
        &self,
        member: &Member,
        task: Option<TaskId>,
        reason: WakeReason,
    ) -> Done<Need> {
        // The leads' standing reviews are judgment whatever woke them; the CEO's are by role.
        let a_review = match task {
            Some(task) if member.role == Role::Lead => self.report_task_ids()?.contains(&task),
            _ => false,
        };
        // A developer in the engineering wing builds with the repo's tools, and its lead plans
        // that code.
        let in_the_wing = matches!(member.role, Role::Developer | Role::Lead)
            && self.wing_members()?.contains(&member.id);
        Ok(Need::of(reason, member.role, a_review, in_the_wing))
    }

    /// What a queued turn needs: the strictest of the wakes that asked for it, its own and every
    /// one folded into it since. A wake of judgment that joined a waiting turn makes it a turn
    /// of judgment.
    pub(super) fn need_of_run(
        &self,
        run: &Run,
        member: &Member,
        wakes: &[Wake],
        known: &Known,
    ) -> Need {
        let a_review =
            member.role == Role::Lead && run.task.is_some_and(|t| known.reports.contains(&t));
        let in_the_wing =
            matches!(member.role, Role::Developer | Role::Lead) && known.wing.contains(&member.id);
        wakes
            .iter()
            .filter(|w| w.id == run.wake || w.run == Some(run.id))
            .map(|w| Need::of(w.reason, member.role, a_review, in_the_wing))
            .reduce(Need::and)
            .unwrap_or_else(|| Need::of(WakeReason::Recovery, member.role, a_review, in_the_wing))
    }

    /// Tells the owner, on the desk, that an alarm turn ran at once on a house model because its
    /// budget was spent (the lock held): once for a turn, however often it is started. The
    /// turn's actions stay gated by the same contracts, whoever thinks it.
    pub(super) fn alarm_on_the_house(&self, run: &Run, member: &Member, brain: Brain) -> Done<()> {
        let key = format!("alarm_notice:{}", run.id.0);
        if self.store.setting(&key)?.is_some() {
            return Ok(());
        }
        self.store.set_setting(&key, "1")?;
        let on = match brain {
            Brain::Mac => "the owner's Mac",
            _ => "the house's own model on the NAS",
        };
        let about = match run.task {
            Some(task) => format!(
                " on {} \"{}\"",
                Ref::Task(task).said(),
                self.task(task)?.title
            ),
            None => String::new(),
        };
        let text = format!(
            "{}'s alarm turn{about} ran at once on {on}, not on Claude, because its budget was \
             spent. It did not wait; what it may do is gated by the same contracts as ever.",
            member.name
        );
        let desk = self.store.add_desk(
            "notice",
            run.task,
            None,
            Some(member.id),
            &cut(&text, 600),
            self.clock.now(),
        )?;
        self.quiet_in_the_loops(desk)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "notice", "task": run.task.map(|t| t.0) }),
        )
    }
}
