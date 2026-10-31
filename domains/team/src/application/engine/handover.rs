//! HQ's handover in the loops (ADR 0142, amended 2026-10-30): when an item changes hands (a
//! climb after three failures, a paused or away member's hand-up, a reviewer's send-back), HQ
//! composes from the record what the next member needs to continue rather than start over
//! ([`handover::compose`]), keeps it for that member's briefings on the item, and posts it on
//! the item's thread as Endora's, so the owner sees what was handed over.
//!
//! Not a member's own handover note (ADR 0134, [`crate::domain::world::HandoverNote`]): that
//! one a member writes as a restart ends its turn; this one is built by code, no model call.

use super::{Author, Done, Engine};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::handover::{self, CriterionSeen, ReadingSeen};
use crate::domain::{Member, MemberId, TaskId};

/// How many of the members' last posts on the item a handover carries.
const TRIED_MOST: usize = 3;
/// How many send-backs, newest first, a handover carries.
const SENT_BACK_MOST: usize = 2;
/// How a send-back begins in a thread, the reviewer's and the checks' alike.
const SENT_BACK: &str = "Sent back:";

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
