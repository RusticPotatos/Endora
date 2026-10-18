//! Discipline (ADR 0124): work split no wider or deeper than it can be followed, each part
//! saying which of its parent's criteria it serves; and proof that is already enough ends in a
//! submission, not more proof.
//!
//! Live, scope grew sub-task by sub-task, each defensible on its own, and members kept
//! re-reading what they had already shown. Neither is a model's to judge: the counts, the
//! criteria and the evidence rule are HQ's record.

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::evidence::{self, Judged, Label, Ref};
use crate::domain::guards::{self, GuardRefusal, THRESHOLDS};
use crate::domain::{MemberId, Millis, Refusal, RunId, Task, TaskId, TaskStatus};

/// Where HQ keeps which of its parent's criteria a sub-task serves.
pub(super) fn serves_key(task: TaskId) -> String {
    format!("serves:{}", task.0)
}

/// Where HQ keeps that a member was told its evidence is enough: the turn told, how many
/// times the work had been sent back then, and how many re-checking turns followed.
fn enough_key(task: TaskId, member: MemberId) -> String {
    format!("enough:{}:{}", task.0, member.0)
}

/// The most receipts cited for one criterion when HQ submits for a member.
const CITED_A_CRITERION: usize = 3;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- sub-task discipline ------------------------------------------------------------------

    /// Whether `parent` may take one more sub-task (ADR 0124): at most five open under it, no
    /// deeper than three, and, under a member's task with criteria, naming which one it
    /// serves. The owner's tasks and a routine's (depth 0) need no `serves`. Returns the
    /// criterion served, if one is.
    pub(super) fn subtask_discipline(
        &self,
        parent: TaskId,
        serves: Option<&str>,
    ) -> Done<Option<usize>> {
        let above = self.task(parent)?;
        let open = self
            .store
            .tasks()?
            .iter()
            .filter(|t| t.parent == Some(parent) && !t.status.finished())
            .count();
        guards::room_for_a_subtask(parent, open, above.depth).map_err(refused)?;
        if above.depth == 0 || above.criteria.is_empty() {
            return Ok(None);
        }
        match serves.and_then(|s| guards::serves(&above.criteria, s)) {
            Some(n) => Ok(Some(n)),
            None => Err(refused(GuardRefusal::ServesWhich {
                parent,
                criteria: above.criteria.iter().map(|c| c.text.clone()).collect(),
            })),
        }
    }

    /// What a briefing says of the criterion a sub-task serves; empty when it serves none.
    pub(crate) fn serves_said(&self, task: &Task) -> Done<String> {
        let (Some(parent), Some(n)) = (
            task.parent,
            self.store
                .setting(&serves_key(task.id))?
                .and_then(|s| s.parse::<usize>().ok()),
        ) else {
            return Ok(String::new());
        };
        let above = self.task(parent)?;
        let Some(criterion) = above.criteria.get(n.saturating_sub(1)) else {
            return Ok(String::new());
        };
        Ok(format!(
            "It serves #{parent}'s criterion {n}: {}. Keep to that.\n",
            criterion.text
        ))
    }

    // ---- evidence that is already enough ------------------------------------------------------

    /// When the work was last sent back, if it was.
    fn last_sent_back(&self, task: TaskId) -> Done<Option<Millis>> {
        let members = self.store.members()?;
        Ok(self
            .store
            .thread(task)?
            .iter()
            .filter(|p| {
                p.text.starts_with(super::evidence::SENT_BACK)
                    && members.iter().any(|m| m.name == p.author)
            })
            .map(|p| p.at)
            .max())
    }

    /// The evidence `member` holds for `task`, per criterion, if it already meets the
    /// criteria by the one evidence rule, `fresh` being the turn whose readings count as
    /// fresh: the member's own readings on the work since it was last sent back, the newest
    /// few for each criterion (a live one's from `fresh` alone), and its time-bound criteria
    /// waited. `None` when it does not, or there is none.
    pub(super) fn enough_evidence(
        &self,
        member: MemberId,
        task: &Task,
        fresh: RunId,
    ) -> Done<Option<Vec<Vec<Ref>>>> {
        let since = self.last_sent_back(task.id)?.unwrap_or(Millis::MIN);
        let work = self.work_of(task.id)?;
        let mut held: Vec<(RunId, i64)> = Vec::new();
        for run in self.store.runs()? {
            if run.member != member || !run.task.is_some_and(|t| work.contains(&t)) {
                continue;
            }
            for r in self.store.call_receipts_of(run.id)? {
                if r.ok && !r.sandbox && r.at > since {
                    held.push((run.id, r.id));
                }
            }
        }
        if held.is_empty() {
            return Ok(None);
        }
        held.sort_by_key(|(_, id)| core::cmp::Reverse(*id));
        let newest = |only: Option<RunId>| -> Vec<Ref> {
            held.iter()
                .filter(|(run, _)| only.is_none_or(|o| o == *run))
                .take(CITED_A_CRITERION)
                .map(|(_, id)| Ref::Receipt(*id))
                .collect()
        };
        let cited: Vec<Vec<Ref>> = if task.criteria.is_empty() {
            vec![newest(None)]
        } else {
            task.criteria
                .iter()
                .map(|c| newest(c.live.then_some(fresh)))
                .collect()
        };
        let found = self.look_up(Some(task), &cited)?;
        let how = Judged {
            fresh: &[fresh],
            own_reading: None,
        };
        if evidence::judge(&task.criteria, &found, &how).is_err() || self.waited_for(task).is_err()
        {
            return Ok(None);
        }
        Ok(Some(cited))
    }

    /// What a member's turn on its own task is told when its evidence already meets the
    /// criteria (ADR 0124), and HQ remembers it was told; empty otherwise. A live criterion
    /// still needs this turn's reading, so it is judged on the member's last turn.
    pub(super) fn enough_said_for(&self, member: MemberId, run: &Run) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        let task = self.task(task)?;
        if task.owner != Some(member) || task.status != TaskStatus::Doing {
            return Ok(String::new());
        }
        let Some(last) = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| {
                r.member == member
                    && r.task == Some(task.id)
                    && r.id != run.id
                    && r.ended_at.is_some()
            })
            .max_by_key(|r| r.id)
        else {
            return Ok(String::new());
        };
        if self.enough_evidence(member, &task, last.id)?.is_none() {
            return Ok(String::new());
        }
        let rounds = self.sent_back(task.id)?;
        let key = enough_key(task.id, member);
        let told = self
            .store
            .setting(&key)?
            .and_then(|kept| read_enough(&kept));
        if told.is_none_or(|(_, then, _)| then != rounds) {
            self.store
                .set_setting(&key, &format!("{}:{rounds}:0", run.id.0))?;
        }
        let live = task.criteria.iter().any(|c| c.live);
        Ok(format!("{}\n", guards::enough_said(live)))
    }

    /// A turn's label, once its member was told its evidence is enough (ADR 0124): a turn
    /// that only checked again, every reading one it had already taken on the task, is talk
    /// only, and is counted. Any other label is as it was.
    pub(super) fn after_enough_label(&self, run: &Run, label: Label) -> Done<Label> {
        if label != Label::Checked {
            return Ok(label);
        }
        let Some(task) = run.task else {
            return Ok(label);
        };
        let key = enough_key(task, run.member);
        let Some((told, rounds, count)) = self
            .store
            .setting(&key)?
            .and_then(|kept| read_enough(&kept))
        else {
            return Ok(label);
        };
        if run.id.0 < told || self.sent_back(task)? != rounds {
            return Ok(label);
        }
        let mut before: Vec<(String, String)> = Vec::new();
        for earlier in self.store.runs()? {
            if earlier.member != run.member || earlier.task != Some(task) || earlier.id >= run.id {
                continue;
            }
            for r in self.store.call_receipts_of(earlier.id)? {
                if r.ok {
                    before.push((r.tool, evidence::same_args(&r.args)));
                }
            }
        }
        let new = self
            .store
            .call_receipts_of(run.id)?
            .into_iter()
            .filter(|r| r.ok)
            .any(|r| !before.contains(&(r.tool, evidence::same_args(&r.args))));
        if new {
            return Ok(label);
        }
        self.store
            .set_setting(&key, &format!("{told}:{rounds}:{}", count + 1))?;
        Ok(Label::TalkOnly)
    }

    /// After a turn that re-checked evidence already enough for the third time (ADR 0124),
    /// HQ submits the work for its member, on that evidence, to the tester and the lead as
    /// any submission goes.
    pub(super) fn submit_when_enough(&self, run: &Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        let Some((_, rounds, count)) = self
            .store
            .setting(&enough_key(task, run.member))?
            .and_then(|kept| read_enough(&kept))
        else {
            return Ok(());
        };
        if count < THRESHOLDS.sufficient_turns || self.sent_back(task)? != rounds {
            return Ok(());
        }
        let current = self.task(task)?;
        if current.owner != Some(run.member) || current.status != TaskStatus::Doing {
            return Ok(());
        }
        let Some(cited) = self.enough_evidence(run.member, &current, run.id)? else {
            return Ok(());
        };
        let name = self.member(run.member)?.name;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "{name}'s evidence met this task's criteria, and {count} turns since only \
                 checked it again: HQ submits it for {name}, on that evidence."
            ),
        )?;
        self.set_status_held_lock(
            run.member,
            task,
            TaskStatus::InReview,
            "submitted by HQ: the evidence already met the criteria",
        )?;
        self.keep_cited(task, run, super::evidence::SUBMITTED, &cited)?;
        let said = self.evidence_said(&cited.concat())?;
        self.post_held_lock(Author::Endora, task, &format!("Evidence cited: {said}"))
    }
}

/// A sufficiency marker as kept: the turn told, the rounds then, the re-checking turns since.
fn read_enough(kept: &str) -> Option<(i64, usize, usize)> {
    let mut parts = kept.split(':').map(str::parse::<i64>);
    let told = parts.next()?.ok()?;
    let rounds = usize::try_from(parts.next()?.ok()?).ok()?;
    let count = usize::try_from(parts.next()?.ok()?).ok()?;
    Some((told, rounds, count))
}

fn refused(r: GuardRefusal) -> EngineError {
    EngineError::Refused(Refusal::Guard(r))
}
