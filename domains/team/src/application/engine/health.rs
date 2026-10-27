//! Process health (ADR 0124, cut down by ADR 0142): the team's talk against its work, kept for
//! the outcomes line; findings on the owner's desk and their verdicts; and the same ask from two
//! tasks put on one desk card. The guards that policed coordination are gone with the company.

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::application::scorecard::Window;
use crate::application::scorecard::outcomes::talk_work;
use crate::domain::guards::{self, Guard, THRESHOLDS, TurnSeen, Verdict};
use crate::domain::{EventKind, MemberId, Millis, TaskId, WakeReason};

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
const CHECKING: [WakeReason; 2] = [WakeReason::ReviewRequested, WakeReason::AcceptanceAsked];
/// How many findings the screen shows.
const FINDINGS_SHOWN: usize = 30;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the tick's look ----------------------------------------------------------------------

    /// The process-health part of the tick, at most every [`THRESHOLDS`]`.look_every_ms`:
    /// the team's talk against its work over the day and the week, and findings that cleared.
    pub(super) fn health_tick(&self) -> Done<()> {
        self.clear_what_moved()?;
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

    /// The same ask clears once the owner answered its card.
    fn clear_what_moved(&self) -> Done<()> {
        let desk = self.store.desk()?;
        for finding in self.store.findings()? {
            if finding.cleared_at.is_some() || finding.guard != Guard::SameAsk {
                continue;
            }
            let item: Option<i64> = finding.key.split(':').nth(1).and_then(|d| d.parse().ok());
            if desk
                .iter()
                .any(|d| Some(d.id.0) == item && d.status != "waiting")
            {
                self.clear(&finding)?;
            }
        }
        Ok(())
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

    /// A question or a decision for the owner goes on the desk, unless the same ask from
    /// another task already waits there (ADR 0124): then it is put with that card ("also on
    /// #672"), said in its own thread, and counted as a finding. Live, the CEO put the same
    /// board decision on the desk twice, once a task, both checks having exited 127.
    pub(super) fn ask_on_the_desk(
        &self,
        kind: &str,
        task: TaskId,
        member: MemberId,
        text: &str,
    ) -> Done<crate::domain::DeskId> {
        let print = crate::domain::retraction::fingerprint(text);
        let waiting = self.store.desk()?.into_iter().find(|d| {
            d.kind == kind
                && d.status == "waiting"
                && d.task.is_some_and(|t| t != task)
                && guards::same_ask(&crate::domain::retraction::fingerprint(&d.text), &print)
        });
        let Some(open) = waiting else {
            let desk = self.store.add_desk(
                kind,
                Some(task),
                None,
                Some(member),
                text,
                self.clock.now(),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": desk.0, "kind": kind, "task": task.0 }),
            )?;
            return Ok(desk);
        };
        let main = open.task.map_or(0, |t| t.0);
        self.store.attach_to_desk(open.id.0, task, Some(member))?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "The same ask as #{main}'s is already on the owner's desk: it is put with it, \
                 and the owner's one answer comes here too."
            ),
        )?;
        let project = self.task(task)?.project.map(|p| p.0);
        // One finding for each task put with a card; it rests on nothing that grows.
        self.find(
            Guard::SameAsk,
            &format!("same_ask:{}:{}", open.id.0, task.0),
            Some(task),
            project,
            &format!(
                "#{}'s {kind} is the same ask as #{main}'s, on the desk as item {}",
                task.0, open.id.0
            ),
            0,
        )?;
        Ok(open.id)
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
            self.post_held_lock(
                Author::Owner,
                task,
                &format!("The owner {said} (the same ask, put with #{main}'s)"),
            )?;
            let Some(to) = member.or(self.task(task)?.owner) else {
                continue;
            };
            self.wake_held_lock(to, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
    }

    // ---- verdicts -----------------------------------------------------------------------------

    /// The owner's verdict on a guard's notice from the desk: the finding is judged, and the
    /// notice put away.
    pub(super) fn desk_verdict(&self, item: crate::domain::DeskId, decision: &str) -> Done<()> {
        if decision == "dismiss" {
            return Ok(());
        }
        let verdict = Verdict::from_name(decision).ok_or(EngineError::Unknown("verdict"))?;
        let Some(finding) = self
            .store
            .findings()?
            .into_iter()
            .find(|f| f.desk == Some(item.0))
        else {
            return Err(EngineError::Unknown("finding"));
        };
        self.judge_held_lock(finding.id, verdict, None, "owner")
    }

    /// The guards as the screen shows them (ADR 0124): every threshold, each guard's
    /// precision, and the newest findings.
    pub(super) fn guards_shown(&self) -> Done<Value> {
        let findings = self.store.findings()?;
        let t = THRESHOLDS;
        let precision: Vec<Value> = guards::precision(&findings)
            .into_iter()
            .map(|p| {
                json!({
                    "guard": p.guard.name(), "plainly": p.guard.plainly(),
                    "rule": p.guard.rule(), "fired": p.fired, "open": p.open,
                    "real": p.real, "resolved": p.resolved,
                    "false_positive": p.false_positive, "not_enough": p.not_enough,
                    "fixed": p.fixed, "fix_judged": p.fix_judged,
                    "precision": p.percent() })
            })
            .collect();
        let shown: Vec<Value> = findings
            .iter()
            .rev()
            .take(FINDINGS_SHOWN)
            .map(|f| {
                json!({ "id": f.id, "guard": f.guard.name(), "plainly": f.guard.plainly(),
                        "task": f.task, "project": f.project, "rule": f.rule,
                        "evidence": f.evidence, "opened_at": f.opened_at,
                        "seen_at": f.seen_at, "episodes": f.episodes,
                        "cleared_at": f.cleared_at, "verdict": f.verdict.map(Verdict::name),
                        "fix_worked": f.fix_worked, "judged_by": f.judged_by })
            })
            .collect();
        Ok(json!({
            "thresholds": {
                "redundant_wakes": t.redundant_wakes,
                "open_subtasks_most": t.open_subtasks_most, "depth_most": t.depth_most,
                "sufficient_turns": t.sufficient_turns,
                "retracted_percent": t.retracted_percent,
                "retracted_shared_least": t.retracted_shared_least,
                "free_turns_a_day": t.free_turns_a_day,
                "runaway_turns": t.runaway_turns,
                "runaway_window_ms": t.runaway_window_ms,
            },
            "precision": precision,
            "findings": shown,
        }))
    }
}
