//! HQ's handover in the loops (ADR 0142, amended 2026-10-30): when an item changes hands (a
//! climb after three failures, a paused or away member's hand-up, a reviewer's send-back), HQ
//! composes from the record what the next member needs to continue rather than start over
//! ([`handover::compose`]), keeps it for that member's briefings on the item, and posts it on
//! the item's thread as Endora's, so the owner sees what was handed over.
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
use crate::domain::{Member, MemberId, Refusal, RunId, TaskId, TaskStatus, WakeReason};

/// How many of the members' last posts on the item a handover carries.
const TRIED_MOST: usize = 3;
/// How many send-backs, newest first, a handover carries.
const SENT_BACK_MOST: usize = 2;
/// How a send-back begins in a thread, the reviewer's and the checks' alike.
const SENT_BACK: &str = "Sent back:";

/// How many times an item goes on with one member in a fresh turn after its turn ran out of
/// room; the next time is one of its failures, and the count starts again.
pub(crate) const CONTINUES_MOST: u32 = 3;

/// Where an item's continuations are kept.
fn continued_key(task: TaskId) -> String {
    format!("loop_continued:{}", task.0)
}

/// An item's continuations: with whom, how many since the count last started, the turn that
/// ran out of room and has not ended yet, and every turn of it that ran out of room.
/// Kept as `{member, n, pending, runs}`.
#[derive(Debug, Default)]
struct Continued {
    member: i64,
    n: u32,
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
            pending: kept["pending"].as_i64(),
            runs: kept["runs"]
                .as_array()
                .map(|r| r.iter().filter_map(serde_json::Value::as_i64).collect())
                .unwrap_or_default(),
        }
    }

    fn written(&self) -> String {
        serde_json::json!({ "member": self.member, "n": self.n, "pending": self.pending,
                            "runs": self.runs })
        .to_string()
    }
}

/// Where an item's latest handover is kept: `{to, text}`.
fn handover_key(task: TaskId) -> String {
    format!("loop_handover:{}", task.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The item goes to `to` (the lock held): its handover is composed within `to`'s model's
    /// size, kept for `to`'s briefings on it, and posted on its thread.
    pub(super) fn hand_over(&self, task: TaskId, to: MemberId, why: &str) -> Done<()> {
        let to = self.member(to)?;
        let composed = handover::compose(
            &self.handover_of(task, &to, why)?,
            handover::chars_for(to.brain),
        );
        let text = composed.trim_end();
        self.store.set_setting(
            &handover_key(task),
            &serde_json::json!({ "to": to.id.0, "text": text }).to_string(),
        )?;
        self.post_held_lock(Author::Endora, task, text)
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
        Ok(handover::Handover {
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
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn not running; [`Refusal::OutOfReach`] for a task that is
    /// not the turn's; a store failure.
    pub fn out_of_room_in(&self, run: RunId, task: TaskId) -> Done<()> {
        let _held = self.held();
        let run = self.running(run)?;
        if run.task != Some(task) {
            return Err(EngineError::Refused(Refusal::OutOfReach));
        }
        let mut kept = self.continued(task)?;
        kept.pending = Some(run.id.0);
        kept.runs.push(run.id.0);
        self.keep_continued(task, &kept)
    }

    /// Whether `run` ended at its prompt's budget: such a turn was working, so it never counts
    /// as one that showed nothing.
    pub(super) fn ran_out_of_room(&self, run: &Run) -> Done<bool> {
        let Some(task) = run.task else {
            return Ok(false);
        };
        Ok(self.continued(task)?.runs.contains(&run.id.0))
    }

    /// After a turn that ran out of room has ended (the lock held): while the item is still its
    /// member's, in the loops and not in review or finished, the member goes on in a fresh turn
    /// with a handover, counting no failure, up to [`CONTINUES_MOST`] times. The next time is
    /// one of the item's failures, as a turn that never finished was, and the count starts
    /// again; if the item is still the member's, it goes on from a fresh handover all the same.
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
        }
        let found = self.task(task)?;
        let still_its = found.owner.or(found.assignee) == Some(run.member)
            && !found.status.finished()
            && found.status != TaskStatus::InReview
            && self.loop_holds(&found)?;
        if !still_its {
            return self.keep_continued(task, &kept);
        }
        if kept.n < CONTINUES_MOST {
            kept.n += 1;
            self.keep_continued(task, &kept)?;
            self.hand_over(
                task,
                run.member,
                &format!(
                    "its turn ran out of room, continuation {} of {CONTINUES_MOST}",
                    kept.n
                ),
            )?;
            self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
            return Ok(());
        }
        kept.n = 0;
        self.keep_continued(task, &kept)?;
        let member = self.member(run.member)?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "{}'s turns on this ran out of room {} times: that is one of its failures.",
                member.name,
                CONTINUES_MOST + 1
            ),
        )?;
        self.loop_failed(task)?;
        let now = self.task(task)?;
        if now.owner.or(now.assignee) != Some(run.member) {
            return Ok(());
        }
        self.hand_over(task, run.member, "its turns kept running out of room")?;
        self.wake_held_lock(run.member, Some(task), WakeReason::Continued)?;
        Ok(())
    }

    /// The handover kept for `member` on `task`, as it was posted, while the item is still
    /// `member`'s.
    pub(super) fn handover_kept_for(&self, member: MemberId, task: TaskId) -> Done<Option<String>> {
        let Some(kept) = self.setting_json::<serde_json::Value>(&handover_key(task))? else {
            return Ok(None);
        };
        if kept["to"].as_i64() != Some(member.0) {
            return Ok(None);
        }
        let found = self.task(task)?;
        if found.owner.or(found.assignee) != Some(member) {
            return Ok(None);
        }
        Ok(kept["text"].as_str().map(str::to_owned))
    }

    /// The item's latest handover, whoever it went to: what HQ handed on of it, for its
    /// reviewer.
    pub(super) fn handover_on(&self, task: TaskId) -> Done<Option<String>> {
        Ok(self
            .setting_json::<serde_json::Value>(&handover_key(task))?
            .and_then(|kept| kept["text"].as_str().map(str::to_owned)))
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
