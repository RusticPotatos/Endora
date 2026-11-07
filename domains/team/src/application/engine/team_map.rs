//! The HQ agents map's read (the owner's decision M5a): members, their turns, the open items
//! they hold and the desk cards waiting on the owner, read from the store and drawn by
//! [`crate::domain::team_map`]. It changes nothing.

use std::collections::HashMap;

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::team_map::{CardSeen, ItemSeen, MemberSeen, Seen, TurnSeen, team_map};
use crate::domain::{RunStatus, WakeReason};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The Core team's map, as the owner's screen reads it.
    ///
    /// # Errors
    /// A store failure.
    pub fn team_map(&self) -> Done<serde_json::Value> {
        let day = self.today();
        let pool_day = self.pool_sized_today()?.base + self.topup_of(day)?;
        let paid = self.paid_weights()?;
        let downtime: Vec<_> = self
            .store
            .wakes()?
            .into_iter()
            .filter(|w| w.reason == WakeReason::Downtime)
            .map(|w| w.id)
            .collect();
        let runs = self.store.runs()?;
        let mut members = Vec::new();
        let all = self.store.members()?;
        for m in &all {
            let mut turns = Vec::new();
            for r in runs.iter().filter(|r| r.member == m.id) {
                let last_signal = if r.status == RunStatus::Running {
                    self.store
                        .call_receipts_of(r.id)?
                        .iter()
                        .map(|c| c.at)
                        .max()
                } else {
                    None
                };
                turns.push(TurnSeen {
                    run: r.id.0,
                    task: r.task.map(|t| t.0),
                    status: r.status.name().to_owned(),
                    started: r.started_at,
                    ended: r.ended_at,
                    downtime: downtime.contains(&r.wake),
                    last_signal,
                });
            }
            let share = self.per_day(m, paid, pool_day);
            members.push(MemberSeen {
                name: m.name.clone(),
                brain: m.brain.label().to_owned(),
                paused: m.paused,
                furloughed: m.furloughed(),
                out_of_budget: share > 0 && self.store.committed(Some(m.id), day)? >= share,
                turns,
            });
        }
        let names: HashMap<_, _> = all.iter().map(|m| (m.id, m.name.clone())).collect();
        let projects: HashMap<_, _> = self
            .store
            .projects()?
            .into_iter()
            .map(|p| (p.id, p.title))
            .collect();
        let reports = self.report_task_ids()?;
        let tasks = self.store.tasks()?;
        let items = tasks
            .iter()
            .filter(|t| !t.status.finished() && !reports.contains(&t.id))
            .map(|t| ItemSeen {
                task: t.id.0,
                title: t.title.clone(),
                status: t.status.name().to_owned(),
                holder: t.assignee.and_then(|a| names.get(&a).cloned()),
                project: t.project.and_then(|p| projects.get(&p).cloned()),
            })
            .collect();
        let project_of = |task| tasks.iter().find(|t| t.id == task).and_then(|t| t.project);
        let desk = self
            .store
            .desk()?
            .into_iter()
            .filter(|d| d.status == "waiting")
            .map(|d| CardSeen {
                id: d.id.0,
                kind: d.kind,
                task: d.task.map(|t| t.0),
                project: d
                    .project
                    .or_else(|| d.task.and_then(project_of))
                    .and_then(|p| projects.get(&p).cloned()),
                at: d.at,
            })
            .collect();
        let seen = Seen {
            now: self.clock.now(),
            members,
            items,
            desk,
        };
        Ok(team_map(&seen).shown())
    }
}
