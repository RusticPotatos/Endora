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
use crate::domain::handover::{
    self, CONTINUES_MOST, Continuation, Continued, CriterionSeen, ReadingSeen,
};
use crate::domain::wing::IN_PROGRESS;
use crate::domain::{Member, MemberId, Refusal, RunId, Runtime, TaskId, TaskStatus, WakeReason};

/// How many of the members' last posts on the item a handover carries.
const TRIED_MOST: usize = 3;
/// How many send-backs, newest first, a handover carries.
const SENT_BACK_MOST: usize = 2;
/// How a send-back begins in a thread, the reviewer's and the checks' alike.
const SENT_BACK: &str = "Sent back:";

/// Where an item's continuations are kept.
fn continued_key(task: TaskId) -> String {
    format!("loop_continued:{}", task.0)
}

/// An item's continuations as kept: `{member, n, total, pending, runs}`.
fn read_continued(kept: &serde_json::Value) -> Continued {
    let count = |key: &str| {
        kept[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    Continued {
        member: kept["member"].as_i64().unwrap_or(0),
        n: count("n"),
        total: count("total"),
        pending: kept["pending"].as_i64(),
        runs: kept["runs"]
            .as_array()
            .map(|r| r.iter().filter_map(serde_json::Value::as_i64).collect())
            .unwrap_or_default(),
    }
}

fn written_continued(kept: &Continued) -> String {
    serde_json::json!({ "member": kept.member, "n": kept.n, "total": kept.total,
                        "pending": kept.pending, "runs": kept.runs })
    .to_string()
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
            .map(|kept| read_continued(&kept))
            .unwrap_or_default())
    }

    fn keep_continued(&self, task: TaskId, kept: &Continued) -> Done<()> {
        self.store
            .set_setting(&continued_key(task), &written_continued(kept))?;
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
    /// with a handover, or it is one of the item's failures, as [`Continued::after_work`]
    /// decides; if the item is still the member's after a failure, it goes on from a fresh
    /// handover all the same. A reviewer's turn goes on as [`Continued::after_review`] decides.
    pub(super) fn after_out_of_room(&self, run: &Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        let mut kept = self.continued(task)?;
        if !kept.ended(run.id.0, run.member.0) {
            return Ok(());
        }
        let found = self.task(task)?;
        if found.status == TaskStatus::InReview
            && self
                .loop_reviewer_of(&found)?
                .is_some_and(|r| r.id == run.member)
        {
            return self.review_goes_on(task, run.member, kept);
        }
        let still_its = found.holder() == Some(run.member)
            && !found.status.finished()
            && found.status != TaskStatus::InReview
            && self.loop_holds(&found)?;
        if !still_its {
            return self.keep_continued(task, &kept);
        }
        let mut branches: Vec<crate::domain::Branch> = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task)
            .collect();
        branches.sort_by_key(|b| b.id);
        let next = kept.after_work(handover::kept_new_work(&branches, run.id.0));
        self.keep_continued(task, &kept)?;
        let said = match next {
            Continuation::GoesOn(why) => {
                self.hand_over(task, run.member, &why)?;
                self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
                return Ok(());
            }
            Continuation::Failure(said) => said,
        };
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
        if now.holder() != Some(run.member) || self.held_above(task)? {
            return Ok(());
        }
        self.hand_over(task, run.member, "its turns kept running out of room")?;
        self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
        Ok(())
    }

    /// A reviewer's turn on the item ran out of room (review of 31 Oct): the reviewer goes on in
    /// a fresh review turn, or the owner is asked about the item ([`Continued::after_review`]):
    /// a review too big for its reviewer is no failure of the worker's.
    fn review_goes_on(&self, task: TaskId, reviewer: MemberId, mut kept: Continued) -> Done<()> {
        let name = self.member(reviewer)?.name;
        let next = kept.after_review();
        self.keep_continued(task, &kept)?;
        if let Some(n) = next {
            self.post_held_lock(
                Author::Endora,
                task,
                &format!(
                    "{name}'s review ran out of room, continuation {n} of {CONTINUES_MOST}: \
                     {name} reviews it afresh."
                ),
            )?;
            self.wake_held_lock(reviewer, Some(task), WakeReason::ReviewRequested)?;
            return Ok(());
        }
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
        if found.holder() != Some(member.id) {
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
