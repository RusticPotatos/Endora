//! HQ's handover in the loops (ADR 0142, amended 2026-10-30): when an item changes hands (a
//! climb after three failures, a paused or away member's hand-up, a reviewer's send-back), HQ
//! composes from the record what the next member needs to continue rather than start over
//! ([`handover::compose`]) and posts it on the item's thread as Endora's, so the owner sees
//! what was handed over. The thread is its one record: the newest handover there is what the
//! member's briefings on the item carry.
//!
//! The same note continues an item with the same member when the worker ended its turn at its
//! prompt's budget (ADR 0142, amended 2026-10-30, after Ralph: every pass fits one fresh context
//! window): the next turn starts from the note, not from the whole conversation, and running
//! out of room is not one of the item's failures until it has happened [`CONTINUES_MOST`] times.
//!
//! Not a member's own handover note (ADR 0134, [`crate::domain::world::HandoverNote`]): that
//! one a member writes as a restart ends its turn; this one is built by code, no model call.

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::handover::{self, CriterionSeen, ReadingSeen};
use crate::domain::wing::IN_PROGRESS;
use crate::domain::{Member, MemberId, Refusal, RunId, Runtime, TaskId, TaskStatus, WakeReason};

/// How many of the members' last posts on the item a handover carries.
const TRIED_MOST: usize = 3;
/// How many send-backs, newest first, a handover carries.
const SENT_BACK_MOST: usize = 2;
/// How a send-back begins in a thread, the reviewer's and the checks' alike.
const SENT_BACK: &str = "Sent back:";

/// How many times an item goes on with one member in a fresh turn after its turn ran out of
/// room keeping no new work; the next time is one of its failures, and the count starts again.
/// A turn that kept new work starts the count again (ADR 0142, amended 2026-10-31).
pub(crate) const CONTINUES_MOST: u32 = 3;

/// How many times in all an item goes on with one member after turns that ran out of room,
/// new work kept or not; the next time is one of its failures, and the count starts again. The
/// loops' money caps bound it too: this keeps one item from taking a member's every turn.
pub(crate) const CONTINUES_IN_ALL: u32 = 12;

/// How many of an item's turns that ran out of room are remembered, newest kept (review of
/// 31 Oct: the list grew without bound). Four a failure, three failures a rank and five ranks
/// come to sixty: a list this long covers an item's whole climb.
const RUNS_KEPT: usize = 64;

/// Where an item's continuations are kept.
fn continued_key(task: TaskId) -> String {
    format!("loop_continued:{}", task.0)
}

/// An item's continuations: with whom, how many in a row kept no new work since the count last
/// started, how many in all, the turn that ran out of room and has not ended yet, and every turn
/// of it that ran out of room. Kept as `{member, n, total, pending, runs}`.
#[derive(Debug, Default)]
struct Continued {
    member: i64,
    n: u32,
    total: u32,
    pending: Option<i64>,
    runs: Vec<i64>,
}

impl Continued {
    fn read(kept: &serde_json::Value) -> Self {
        Self {
            member: kept["member"].as_i64().unwrap_or(0),
            n: kept["n"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            total: kept["total"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            pending: kept["pending"].as_i64(),
            runs: kept["runs"]
                .as_array()
                .map(|r| r.iter().filter_map(serde_json::Value::as_i64).collect())
                .unwrap_or_default(),
        }
    }

    /// `run` ran out of room: it is pending, and remembered among the newest [`RUNS_KEPT`].
    fn ran_out(&mut self, run: i64) {
        self.pending = Some(run);
        self.runs.push(run);
        let over = self.runs.len().saturating_sub(RUNS_KEPT);
        self.runs.drain(..over);
    }

    fn written(&self) -> String {
        serde_json::json!({ "member": self.member, "n": self.n, "total": self.total,
                            "pending": self.pending, "runs": self.runs })
        .to_string()
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The item goes to `to` (the lock held): its handover is composed within `to`'s model's
    /// size and posted on its thread, where `to`'s briefings on it find it.
    pub(super) fn hand_over(&self, task: TaskId, to: MemberId, why: &str) -> Done<()> {
        let to = self.member(to)?;
        let composed = handover::compose(
            &self.handover_of(task, &to, why)?,
            handover::chars_for(to.brain),
        );
        self.post_held_lock(Author::Endora, task, composed.trim_end())
    }

    /// What the record says of an item for its handover to `to`. Only this item's record:
    /// its criteria, the evidence cited on it, its thread, and the readings of its turns.
    fn handover_of(&self, task: TaskId, to: &Member, why: &str) -> Done<handover::Handover> {
        let found = self.task(task)?;
        let thread = self.store.thread(task)?;
        let members = self.store.members()?;
        let is_member = |author: &str| members.iter().any(|m| m.name == author);
        let sent_back: Vec<&crate::application::ports::Post> = thread
            .iter()
            .filter(|p| p.text.starts_with(SENT_BACK))
            .collect();
        let cited = self.store.cited(task)?;
        // Work with no criteria is cited as one whole (criterion 0): said by its title.
        let asks: Vec<(usize, String)> = if found.criteria.is_empty() {
            vec![(0, found.title.clone())]
        } else {
            found
                .criteria
                .iter()
                .enumerate()
                .map(|(i, c)| (i + 1, c.text.clone()))
                .collect()
        };
        let criteria = asks
            .into_iter()
            .map(|(n, text)| {
                let last = cited.iter().rev().find(|k| k.criterion == n);
                CriterionSeen {
                    n,
                    text,
                    cited: last
                        .map(|k| k.refs.iter().map(|r| r.said()).collect())
                        .unwrap_or_default(),
                    sent_back_since: last.is_some_and(|k| sent_back.iter().any(|p| p.at >= k.at)),
                }
            })
            .collect();
        let reasons = sent_back
            .iter()
            .rev()
            .take(SENT_BACK_MOST)
            .map(|p| {
                let who = if is_member(&p.author) {
                    p.author.as_str()
                } else {
                    "the checks"
                };
                format!("{who}: {}", p.text[SENT_BACK.len()..].trim())
            })
            .collect();
        // The readings of every turn on the item, the newest of each tool and arguments.
        let mut readings: Vec<crate::domain::CallReceipt> = Vec::new();
        for run in self.store.runs_of(task)? {
            readings.extend(
                self.store
                    .call_receipts_of(run.id)?
                    .into_iter()
                    .filter(|r| r.task == Some(task)),
            );
        }
        readings.sort_by_key(|r| std::cmp::Reverse(r.id));
        let mut seen: Vec<(String, String)> = Vec::new();
        readings.retain(|r| {
            let key = (r.tool.clone(), r.args.clone());
            let first = !seen.contains(&key);
            seen.push(key);
            first
        });
        let readings = readings
            .into_iter()
            .map(|r| ReadingSeen {
                said: r.said(),
                tool: r.tool,
                args: r.args,
                ok: r.ok,
                excerpt: r.excerpt,
            })
            .collect();
        let tried: Vec<(String, String)> = thread
            .iter()
            .filter(|p| is_member(&p.author) && !p.text.starts_with(SENT_BACK))
            .map(|p| (p.author.clone(), p.text.clone()))
            .collect();
        let tried = tried[tried.len().saturating_sub(TRIED_MOST)..].to_vec();
        // Where the work is kept, for the next turn to check out (ADR 0142, amended
        // 2026-10-31).
        let kept = self.newest_branch(task)?.map(|b| handover::KeptWork {
            in_progress: b.status == IN_PROGRESS,
            branch: b.name,
            head: b.head,
        });
        Ok(handover::Handover {
            kept,
            task: task.0,
            title: found.title,
            to: to.name.clone(),
            why: why.to_owned(),
            criteria,
            sent_back: reasons,
            readings,
            tried,
        })
    }

    fn continued(&self, task: TaskId) -> Done<Continued> {
        Ok(self
            .setting_json::<serde_json::Value>(&continued_key(task))?
            .map(|kept| Continued::read(&kept))
            .unwrap_or_default())
    }

    fn keep_continued(&self, task: TaskId, kept: &Continued) -> Done<()> {
        self.store
            .set_setting(&continued_key(task), &kept.written())?;
        Ok(())
    }

    /// The worker says its turn on `task` ended at its prompt's budget: once the turn has ended,
    /// the item goes on with the same member in a fresh turn ([`Self::after_out_of_room`]).
    /// HQ decides, not the mark ([`handover::out_of_room`]): a Claude Code turn, or one the
    /// relay metered far below its brain's budget, is not continued. Returns whether it will be.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn not running; [`Refusal::OutOfReach`] for a task that is
    /// not the turn's; a store failure.
    pub fn out_of_room_in(&self, run: RunId, task: TaskId) -> Done<bool> {
        let _held = self.held();
        let run = self.running(run)?;
        if run.task != Some(task) {
            return Err(EngineError::Refused(Refusal::OutOfReach));
        }
        let brain = match run.brain {
            Some(brain) => brain,
            None => self.member(run.member)?.brain,
        };
        let claude_code = self.runtime_of_turn(run.id)? == Runtime::ClaudeCode;
        let metered = run.input_tokens + run.cache_read_tokens + run.cache_write_tokens;
        if !handover::out_of_room(brain, claude_code, metered) {
            return Ok(false);
        }
        let mut kept = self.continued(task)?;
        kept.ran_out(run.id.0);
        self.keep_continued(task, &kept)?;
        Ok(true)
    }

    /// Whether `run` ended at its prompt's budget: such a turn was working, so it never counts
    /// as one that showed nothing.
    pub(super) fn ran_out_of_room(&self, run: &Run) -> Done<bool> {
        let Some(task) = run.task else {
            return Ok(false);
        };
        Ok(self.out_of_room_runs(task)?.contains(&run.id.0))
    }

    /// After a turn that ran out of room has ended (the lock held): while the item is still its
    /// member's, in the loops and not in review or finished, the member goes on in a fresh turn
    /// with a handover, counting no failure. A turn that kept new work on the item's branch
    /// ([`Self::kept_new_work`]) was making progress, and starts the count again (ADR 0142,
    /// amended 2026-10-31); turns that kept nothing go on up to [`CONTINUES_MOST`] times in a
    /// row, and [`CONTINUES_IN_ALL`] times in all. Past either, it is one of the item's
    /// failures, as a turn that never finished was, and the counts start again; if the item is
    /// still the member's, it goes on from a fresh handover all the same.
    pub(super) fn after_out_of_room(&self, run: &Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        let mut kept = self.continued(task)?;
        if kept.pending != Some(run.id.0) {
            return Ok(());
        }
        kept.pending = None;
        if kept.member != run.member.0 {
            kept.member = run.member.0;
            kept.n = 0;
            kept.total = 0;
        }
        let found = self.task(task)?;
        if found.status == TaskStatus::InReview
            && self
                .loop_reviewer_of(&found)?
                .is_some_and(|r| r.id == run.member)
        {
            return self.review_goes_on(task, run.member, kept);
        }
        let still_its = found.owner.or(found.assignee) == Some(run.member)
            && !found.status.finished()
            && found.status != TaskStatus::InReview
            && self.loop_holds(&found)?;
        if !still_its {
            return self.keep_continued(task, &kept);
        }
        let progress = self.kept_new_work(run, task)?;
        if progress {
            kept.n = 0;
        }
        if kept.n < CONTINUES_MOST && kept.total < CONTINUES_IN_ALL {
            kept.total += 1;
            let why = if progress {
                format!(
                    "its turn ran out of room having kept new work, continuation {} in all",
                    kept.total
                )
            } else {
                kept.n += 1;
                format!(
                    "its turn ran out of room, continuation {} of {CONTINUES_MOST}",
                    kept.n
                )
            };
            self.keep_continued(task, &kept)?;
            self.hand_over(task, run.member, &why)?;
            self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
            return Ok(());
        }
        let said = if kept.total >= CONTINUES_IN_ALL {
            format!("{CONTINUES_IN_ALL} continuations in all")
        } else {
            format!("{} times without keeping new work", CONTINUES_MOST + 1)
        };
        kept.n = 0;
        kept.total = 0;
        self.keep_continued(task, &kept)?;
        let member = self.member(run.member)?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "{}'s turns on this ran out of room {said}: that is one of its failures.",
                member.name
            ),
        )?;
        self.loop_failed(task)?;
        let now = self.task(task)?;
        if now.owner.or(now.assignee) != Some(run.member) || self.held_above(task)? {
            return Ok(());
        }
        self.hand_over(task, run.member, "its turns kept running out of room")?;
        self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
        Ok(())
    }

    /// Whether `run` kept new work on `task`: a branch it recorded (submitted, or kept in
    /// progress as it ran out of room) whose head is not the head the item's branch had before.
    /// HQ's record decides, not the turn's word.
    fn kept_new_work(&self, run: &Run, task: TaskId) -> Done<bool> {
        let mut branches: Vec<crate::domain::Branch> = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task)
            .collect();
        branches.sort_by_key(|b| b.id);
        let Some(first) = branches.iter().position(|b| b.run == run.id) else {
            return Ok(false);
        };
        let Some(newest) = branches.iter().rfind(|b| b.run == run.id) else {
            return Ok(false);
        };
        let before = first.checked_sub(1).map(|i| branches[i].head.as_str());
        Ok(before != Some(newest.head.as_str()))
    }

    /// A reviewer's turn on the item ran out of room (review of 31 Oct): the reviewer goes on in
    /// a fresh review turn, up to [`CONTINUES_MOST`] times. The next time the owner is asked
    /// about the item, and the count starts again: a review too big for its reviewer is no
    /// failure of the worker's.
    fn review_goes_on(&self, task: TaskId, reviewer: MemberId, mut kept: Continued) -> Done<()> {
        let name = self.member(reviewer)?.name;
        if kept.n < CONTINUES_MOST {
            kept.n += 1;
            self.keep_continued(task, &kept)?;
            self.post_held_lock(
                Author::Endora,
                task,
                &format!(
                    "{name}'s review ran out of room, continuation {} of {CONTINUES_MOST}: \
                     {name} reviews it afresh.",
                    kept.n
                ),
            )?;
            self.wake_held_lock(reviewer, Some(task), WakeReason::ReviewRequested)?;
            return Ok(());
        }
        kept.n = 0;
        self.keep_continued(task, &kept)?;
        let text = format!(
            "{name}'s reviews of #{} ran out of room {} times. Read it yourself, or move it \
             back to work.",
            task.0,
            CONTINUES_MOST + 1
        );
        self.ask_about_the_item(task, None, &text)
    }

    /// The handover for `member` on `task`, as it was posted: the item's newest, when it went
    /// to `member` and the item is still theirs.
    pub(super) fn handover_kept_for(&self, member: MemberId, task: TaskId) -> Done<Option<String>> {
        let Some(text) = self.handover_on(task)? else {
            return Ok(None);
        };
        let member = self.member(member)?;
        if !handover::is_to(&text, task.0, &member.name) {
            return Ok(None);
        }
        let found = self.task(task)?;
        if found.owner.or(found.assignee) != Some(member.id) {
            return Ok(None);
        }
        Ok(Some(text))
    }

    /// The item's newest handover on its thread, whoever it went to: what HQ handed on of it,
    /// for its reviewer.
    pub(super) fn handover_on(&self, task: TaskId) -> Done<Option<String>> {
        Ok(self
            .store
            .thread(task)?
            .into_iter()
            .rfind(|p| handover::is_handover(&p.author, &p.text))
            .map(|p| p.text))
    }

    /// The item's turns HQ judged out of room ([`Self::out_of_room_in`]): the one flag the
    /// thread view and [`Self::ran_out_of_room`] read.
    pub(super) fn out_of_room_runs(&self, task: TaskId) -> Done<Vec<i64>> {
        Ok(self.continued(task)?.runs)
    }

    /// A briefing's handover section: the item's latest handover, when it was to this turn's
    /// member and the item is still theirs. It carries members' words, so the turn's input is
    /// marked as of unknown origin.
    pub(super) fn handover_briefed(&self, member: &Member, run: &Run) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        let Some(text) = self.handover_kept_for(member.id, task)? else {
            return Ok(String::new());
        };
        self.unclassified_message_input(run.id)?;
        Ok(format!("{text}\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_turns_out_of_room_are_remembered() {
        let mut kept = Continued::default();
        for run in 1..=100 {
            kept.ran_out(run);
        }
        assert_eq!(kept.pending, Some(100));
        assert_eq!(kept.runs.len(), RUNS_KEPT);
        assert_eq!(kept.runs.first(), Some(&(100 - RUNS_KEPT as i64 + 1)));
        let again = Continued::read(&serde_json::from_str(&kept.written()).unwrap());
        assert_eq!(again.runs, kept.runs);
    }
}
