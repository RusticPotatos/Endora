//! Process health (ADR 0124, cut down by ADR 0142): the team's talk against its work, kept for
//! the outcomes line; findings on the owner's desk and their verdicts; and the same ask from two
//! tasks put on one desk card. The guards that policed coordination are gone with the company.

use serde_json::json;

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::application::scorecard::Window;
use crate::application::scorecard::outcomes::talk_work;
use crate::domain::guards::{self, THRESHOLDS, TurnSeen};
use crate::domain::{EventKind, MemberId, Millis, Refusal, TaskId, WakeReason, desk};

/// Where HQ keeps when it last looked for process-health findings.
const LOOKED_AT: &str = "guards_looked_at";
/// Where HQ keeps the whole team's talk against work as it last looked, for the outcomes line
/// (ADR 0132): the same labels as the ceremony, over the same day and week.
pub(super) const TALK_SEEN: &str = "guards_talk";

/// One ended turn, as the process-health look reads it.
pub(super) struct Seen {
    /// The task it was on.
    pub(super) task: TaskId,
    /// What it did.
    pub(super) turn: TurnSeen,
    /// When each act of its that succeeded was done.
    pub(super) acts: Vec<Millis>,
}

/// The wakes that ask for a review round: coordination by their nature.
const CHECKING: [WakeReason; 1] = [WakeReason::ReviewRequested];

/// Where HQ keeps the owner's newest answer in words to a question about `task` (ADR 0142,
/// amended 2026-11-01).
fn answered_key(task: TaskId) -> String {
    format!("answered:{}", task.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the tick's look ----------------------------------------------------------------------

    /// The process-health part of the tick, at most every [`THRESHOLDS`]`.look_every_ms`:
    /// the team's talk against its work over the day and the week, and findings that cleared.
    pub(super) fn health_tick(&self) -> Done<()> {
        let now = self.clock.now();
        let looked: Millis = self
            .store
            .setting(LOOKED_AT)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(Millis::MIN);
        if now.saturating_sub(looked) < THRESHOLDS.look_every_ms {
            return Ok(());
        }
        self.store.set_setting(LOOKED_AT, &now.to_string())?;
        let turns = self.turns_seen(now, None)?;
        self.look_at_talk(now, &turns)
    }

    /// What each ended turn of the last week did, for the ceremony detector and the outcomes
    /// line, with the project of its task if it has one; only those on `only`, when a task is
    /// named (ADR 0133: a task's budget of talk is asked about one task at a time).
    pub(super) fn turns_seen(&self, now: Millis, only: Option<TaskId>) -> Done<Vec<Seen>> {
        let wakes = self.store.wakes()?;
        let branches = self.store.branches()?;
        let desk = self.store.desk()?;
        let week = guards::WINDOWS[1].0;
        let mut runs: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.started_at.is_some() && r.label.is_some())
            .filter(|r| only.is_none_or(|task| r.task == Some(task)))
            .collect();
        runs.sort_by_key(|r| r.id);
        let mut seen_readings: Vec<(TaskId, String, String)> = Vec::new();
        let mut turns = Vec::new();
        for run in &runs {
            let (Some(task), Some(label), Some(ended)) = (run.task, run.label, run.ended_at) else {
                continue;
            };
            let receipts = self.store.call_receipts_of(run.id)?;
            let readings: Vec<(TaskId, String, String)> = receipts
                .iter()
                .filter(|r| r.ok && !r.act)
                .map(|r| {
                    (
                        task,
                        r.tool.clone(),
                        crate::domain::evidence::same_args(&r.args),
                    )
                })
                .collect();
            let new_evidence = readings.iter().any(|r| !seen_readings.contains(r));
            seen_readings.extend(readings);
            if now - ended >= week {
                continue;
            }
            let reason = wakes.iter().find(|w| w.id == run.wake).map(|w| w.reason);
            let started = run.started_at.unwrap_or(ended);
            let asked =
                desk.iter().any(|d| {
                    d.member == Some(run.member)
                        && d.task == Some(task)
                        && started <= d.at
                        && d.at <= ended
                }) || self.store.thread(task)?.iter().any(|p| {
                    p.text.starts_with("Question for ") && started <= p.at && p.at <= ended
                });
            turns.push(Seen {
                task,
                turn: TurnSeen {
                    ended_at: ended,
                    label,
                    checking: reason.is_some_and(|r| CHECKING.contains(&r)),
                    acted: receipts.iter().any(|r| r.ok && r.act),
                    branch: branches.iter().any(|b| b.run == run.id),
                    criterion_met: self.store.cited(task)?.iter().any(|c| c.run == run.id),
                    new_evidence,
                    asked,
                    spent: run.spent,
                },
                acts: receipts
                    .iter()
                    .filter(|r| r.ok && r.act)
                    .map(|r| r.at)
                    .collect(),
            });
        }
        Ok(turns)
    }

    /// The whole team's talk against work over the day and the week, with the ceremony guard's
    /// own labels, kept for the outcomes line (ADR 0132): turns that coordinated against turns
    /// that changed the product, the posts members wrote and the acts that succeeded. A lead's
    /// report and patterns turns are the lead's standing reviews, not talk about work.
    fn look_at_talk(&self, now: Millis, turns: &[Seen]) -> Done<()> {
        let reports = self.report_task_ids()?;
        let mine: Vec<&Seen> = turns
            .iter()
            .filter(|s| !reports.contains(&s.task))
            .collect();
        let seen: Vec<TurnSeen> = mine.iter().map(|s| s.turn).collect();
        let acts: Vec<Millis> = mine.iter().flat_map(|s| s.acts.iter().copied()).collect();
        let week = guards::WINDOWS[1].0;
        let members: std::collections::HashSet<String> =
            self.store.members()?.into_iter().map(|m| m.name).collect();
        let mut posts = Vec::new();
        for task in self.store.tasks()? {
            if reports.contains(&task.id) {
                continue;
            }
            posts.extend(
                self.store
                    .thread(task.id)?
                    .into_iter()
                    .filter(|p| members.contains(&p.author) && now - p.at < week)
                    .map(|p| p.at),
            );
        }
        let over = |ms: Millis| talk_work(&seen, &posts, &acts, &Window::rolling(now, ms)).json();
        let kept = json!({ "at": now, "day": over(guards::WINDOWS[0].0), "week": over(week) });
        self.store.set_setting(TALK_SEEN, &kept.to_string())?;
        Ok(())
    }

    // ---- the same ask, once on the desk -------------------------------------------------------

    /// Keeps the owner's answer in words to a question about `task`, the newest one standing.
    pub(super) fn keep_the_answer(&self, task: TaskId, words: &str) -> Done<()> {
        let words = words.trim();
        if words.is_empty() {
            return Ok(());
        }
        self.store
            .set_setting(&answered_key(task), &crate::domain::cut::cut(words, 600))?;
        Ok(())
    }

    /// A member's question about `task` once the owner has answered one about it in words
    /// (ADR 0142, amended 2026-11-01): refused with the answer, so the member acts on it. An
    /// approval for a step that cannot be undone, a block, and HQ's own asks are not questions
    /// of a member's and still reach the desk.
    ///
    /// # Errors
    /// [`Refusal::Answered`] with the owner's answer; a store failure.
    pub(super) fn not_asked_again(&self, task: TaskId) -> Done<()> {
        let Some(words) = self.store.setting(&answered_key(task))? else {
            return Ok(());
        };
        Err(EngineError::Refused(Refusal::Answered { words }))
    }

    /// An ask about `task` goes on the owner's desk as [`desk::placing`] says (ADR 0124; ADR
    /// 0142, amended 2026-10-31): a card of its own, from `member` when a member raised it;
    /// added to the item's question already waiting; or put with another task's card that asks
    /// the same, said in its own thread. `from_a_turn` for a member's own ask in its turn.
    ///
    /// # Errors
    /// [`Refusal::QuestionWaits`] for a member's question about an item whose question waits.
    pub(super) fn put_on_the_desk(
        &self,
        kind: &str,
        task: TaskId,
        member: Option<MemberId>,
        text: &str,
        from_a_turn: bool,
    ) -> Done<crate::domain::DeskId> {
        let cards = self.store.desk()?;
        let waiting: Vec<desk::Waiting<'_>> = cards
            .iter()
            .filter(|d| d.status == "waiting")
            .map(|d| desk::Waiting {
                id: d.id,
                kind: &d.kind,
                task: d.task,
                text: &d.text,
            })
            .collect();
        match desk::placing(kind, task, from_a_turn, text, &waiting) {
            desk::Placing::Refused(item) => Err(EngineError::Refused(Refusal::QuestionWaits {
                item: item.0,
            })),
            desk::Placing::AddedTo(item) => {
                let said = cards
                    .iter()
                    .find(|d| d.id == item)
                    .map_or("", |d| d.text.as_str());
                self.store.set_desk_text(item, &format!("{said}\n{text}"))?;
                Ok(item)
            }
            desk::Placing::SameAs(item) => {
                let main = waiting
                    .iter()
                    .find(|d| d.id == item)
                    .and_then(|d| d.task)
                    .map_or(0, |t| t.0);
                self.store.attach_to_desk(item.0, task, member)?;
                self.post_held_lock(
                    Author::Endora,
                    task,
                    &format!(
                        "The same ask as #{main}'s is already on the owner's desk: it is put \
                         with it, and the owner's one answer comes here too."
                    ),
                )?;
                Ok(item)
            }
            desk::Placing::New => {
                let item =
                    self.store
                        .add_desk(kind, Some(task), None, member, text, self.clock.now())?;
                self.emit(
                    EventKind::DeskAdded,
                    json!({ "item": item.0, "kind": kind, "task": task.0 }),
                )?;
                Ok(item)
            }
        }
    }

    /// The owner's one answer to an ask, delivered to every task put with it (ADR 0124): in
    /// each one's thread, as the owner's, and whoever asked there woken to read it.
    pub(super) fn answer_the_same_asks(
        &self,
        item: crate::domain::DeskId,
        decision: &str,
        words: &str,
    ) -> Done<()> {
        let attached = self.store.attached(item.0)?;
        if attached.is_empty() {
            return Ok(());
        }
        let Some(found) = self.store.desk()?.into_iter().find(|d| d.id == item) else {
            return Ok(());
        };
        let main = found.task.map_or(0, |t| t.0);
        let said = match (decision, words.trim()) {
            ("approve", "") => "approved".to_owned(),
            ("decline", "") => "declined".to_owned(),
            ("approve", w) => format!("approved: {w}"),
            ("decline", w) => format!("declined: {w}"),
            (_, "") => "seen".to_owned(),
            (_, w) => format!("answered: {w}"),
        };
        for (task, member) in attached {
            if decision == "seen" {
                self.keep_the_answer(task, words)?;
            }
            self.post_held_lock(
                Author::Owner,
                task,
                &format!("The owner {said} (the same ask, put with #{main}'s)"),
            )?;
            let Some(to) = member.or(self.task(task)?.owner) else {
                continue;
            };
            // A blocked item in the loops is woken only by the owner's words.
            if self.waits_for_the_owner(task, WakeReason::ProposalDecided, Some(words))? {
                continue;
            }
            self.wake_held_lock(to, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
    }

    // ---- verdicts -----------------------------------------------------------------------------
}
