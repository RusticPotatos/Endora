//! The Live Work map's read (proposal v3 §14; the owner's decision M3): one item's turns,
//! refused wakes, branches, checks, desk cards, handovers and closure, read from the store and
//! drawn by [`crate::domain::live_work`]. It changes nothing.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, Wake, WorkerAdapter};
use crate::domain::live_work::{
    BranchSeen, CheckSeen, ClosureSeen, DeskSeen, HandoverSeen, RefusedSeen, RunSeen, Seen,
    live_map,
};
use crate::domain::{MemberId, Millis, Receipt, TaskId};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// One item's Live Work map, as the owner's screen reads it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for no such task; a store failure.
    pub fn task_live(&self, id: TaskId) -> Done<serde_json::Value> {
        let tasks = self.store.tasks()?;
        let task = tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or(EngineError::Unknown("task"))?;
        let members = self.store.members()?;
        let name = |m: MemberId| {
            members
                .iter()
                .find(|x| x.id == m)
                .map_or_else(|| "someone".to_owned(), |x| x.name.clone())
        };
        let wakes = self.store.wakes()?;
        let (runs, last_signal) = self.live_runs(id, &wakes, &name)?;
        let refused = refused_wakes(id, &wakes, &name);
        let branches = self.store.branches()?;
        let seen = Seen {
            now: self.clock.now(),
            status: task.status.name().to_owned(),
            assignee: task.assignee.map(name),
            closure: self.store.closure(id)?.map(|c| ClosureSeen {
                why: c.closure.name().to_owned(),
                said: c.closure.plainly(),
                at: c.at,
            }),
            runs,
            refused,
            branches: branches
                .iter()
                .filter(|b| b.task == id)
                .map(|b| BranchSeen {
                    name: b.name.clone(),
                    files: b.files,
                    plus: b.plus,
                    minus: b.minus,
                    at: b.at,
                    run: b.run.0,
                    who: name(b.member),
                })
                .collect(),
            checks: self
                .store
                .checks()?
                .into_iter()
                .filter(|c| c.task == id)
                .map(|c| CheckSeen {
                    id: c.id,
                    ok: c.ok,
                    summary: c.summary.clone(),
                    at: c.done_at.unwrap_or(c.queued_at),
                    run: c.run.0,
                })
                .collect(),
            desk: self
                .store
                .desk()?
                .into_iter()
                .filter(|d| d.task == Some(id))
                .map(|d| DeskSeen {
                    id: d.id.0,
                    kind: d.kind,
                    text: d.text,
                    status: d.status,
                    at: d.at,
                    from: d.member.map(name),
                })
                .collect(),
            handovers: self
                .store
                .handovers(id)?
                .into_iter()
                .map(|h| HandoverSeen {
                    from: h.from.map(name),
                    to: h.to.map(name),
                    at: h.at,
                })
                .collect(),
            last_signal,
        };
        let mut shown = live_map(&seen).shown();
        shown["task"] = serde_json::json!({ "id": id.0, "title": task.title,
            "status": task.status.name(), "assignee": seen.assignee });
        Ok(shown)
    }

    /// The item's turns, and the latest sign of life from one still running.
    fn live_runs(
        &self,
        id: TaskId,
        wakes: &[Wake],
        name: &dyn Fn(MemberId) -> String,
    ) -> Done<(Vec<RunSeen>, Option<Millis>)> {
        let mut runs = Vec::new();
        let mut last_signal = None;
        for r in self.store.runs_of(id)? {
            let receipts = self.store.call_receipts_of(r.id)?;
            if r.status.live() {
                last_signal = last_signal.max(receipts.iter().map(|c| c.at).max());
            }
            let why = wakes
                .iter()
                .find(|w| w.id == r.wake)
                .map(|w| w.reason.name().to_owned())
                .unwrap_or_default();
            runs.push(RunSeen {
                id: r.id.0,
                who: name(r.member),
                why,
                status: r.status.name().to_owned(),
                started: r.started_at,
                ended: r.ended_at,
                end_reason: r.end_reason.clone(),
                receipts: receipts.len(),
                moved_forward: r.moved_forward,
            });
        }
        Ok((runs, last_signal))
    }
}

/// The wakes on the item that were turned away, and why.
fn refused_wakes(
    id: TaskId,
    wakes: &[Wake],
    name: &dyn Fn(MemberId) -> String,
) -> Vec<RefusedSeen> {
    wakes
        .iter()
        .filter(|w| w.task == Some(id))
        .filter_map(|w| match w.receipt {
            Receipt::Refused(why) => Some(RefusedSeen {
                at: w.at,
                who: name(w.member),
                reason: w.reason.name().to_owned(),
                refused: why.name().to_owned(),
            }),
            _ => None,
        })
        .collect()
}
