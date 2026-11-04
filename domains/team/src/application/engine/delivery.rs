//! How accepted work reaches the owner's fork without a card (ADR 0115, amended 2026-10-31).
//! An accepted branch that passed its review, its check and HQ's mechanical checks is due to be
//! pushed: the node asks what is due, scans the branch's whole diff and commit messages for
//! anything personal, pushes it with its deploy key when the scan is clean, and says what came of
//! it. A scan that finds anything pushes nothing and puts one card on the desk. With merging on,
//! a pushed branch is merged into the fork's develop the same way. Accepted branches are listed on
//! Manage until merged. The rules are [`crate::domain::delivery`]'s.

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, TeamStore, WorkerAdapter};
use crate::domain::delivery::{self, Found, Passed};
use crate::domain::{Branch, CallReceipt, EventKind, Refusal, TaskStatus, WakeReason};

/// A branch the node is to push, and whether its privacy scan still decides: not when the owner
/// read what it found and said to push it anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// The branch.
    pub branch: Branch,
    /// Whether the node scans it first.
    pub scan: bool,
}

/// What the node is to do now: branches to push, and pushed branches to merge into develop
/// (only while merging is on).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deliveries {
    /// To push.
    pub push: Vec<Due>,
    /// To merge into the fork's develop.
    pub merge: Vec<Branch>,
}

/// What the node says came of a branch it was asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivered {
    /// It pushed (or tried): what git said, and what the change needs of the owner.
    Pushed {
        /// Whether it is on the fork.
        ok: bool,
        /// What came of it, in a line.
        text: String,
        /// What the change needs of the owner (a vault variable a role now reads, a variable
        /// an assert now requires), found by the node's code.
        needs: Vec<Found>,
    },
    /// The privacy scan found something: nothing was pushed.
    Held {
        /// What and where, never the value.
        found: Vec<Found>,
    },
    /// It merged the pushed branch into develop (or tried).
    Merged {
        /// Whether develop holds it.
        ok: bool,
        /// What came of it, in a line.
        text: String,
    },
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Whether the owner put the "Ready to merge" and "Push" cards back on the desk.
    pub(super) fn cards_back(&self) -> Done<bool> {
        Ok(self.store.setting(delivery::CARDS)?.as_deref() == Some("on"))
    }

    /// Whether HQ merges a pushed branch into the fork's develop.
    fn merging(&self) -> Done<bool> {
        Ok(self.store.setting(delivery::MERGE_DEVELOP)?.as_deref() == Some("on"))
    }

    /// The owner's two settings: the cards back on the desk, and merging develop. `None` leaves
    /// one as it is.
    ///
    /// # Errors
    /// A store failure.
    pub fn set_delivery(&self, cards: Option<bool>, merge_develop: Option<bool>) -> Done<()> {
        let _held = self.held();
        let word = |on: bool| if on { "on" } else { "off" };
        if let Some(on) = cards {
            self.store.set_setting(delivery::CARDS, word(on))?;
        }
        if let Some(on) = merge_develop {
            self.store.set_setting(delivery::MERGE_DEVELOP, word(on))?;
        }
        Ok(())
    }

    /// The two settings as the screen shows them.
    pub(super) fn delivery_shown(&self) -> Done<Value> {
        Ok(json!({ "cards": self.cards_back()?, "merge_develop": self.merging()? }))
    }

    fn kept_of(&self, branch: i64) -> Done<Value> {
        Ok(self
            .setting_json::<Value>(&format!("{}{branch}", delivery::KEPT))?
            .unwrap_or_else(|| json!({})))
    }

    fn keep(&self, branch: i64, why: &str) -> Done<()> {
        let mut kept = self.kept_of(branch)?;
        if kept["at"].as_i64().is_none() {
            kept["at"] = json!(self.clock.now());
        }
        kept["why"] = json!(why);
        self.store
            .set_setting(&format!("{}{branch}", delivery::KEPT), &kept.to_string())?;
        Ok(())
    }

    /// An accepted branch, with no card: due to be pushed when every code check passed, else
    /// listed with why not (the lock held).
    pub(super) fn accept_branch(&self, mut branch: Branch) -> Done<()> {
        let task = self.task(branch.task)?;
        let report = self.mechanical_checks(&task)?;
        let check = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == branch.id && c.status == "done")
            .max_by_key(|c| c.id)
            .map(|c| c.ok == Some(true));
        let passed = Passed {
            can_push: branch.can_push,
            check,
            forbidden: report.forbidden(),
            no_ci_run: report.no_ci_run().is_some(),
        };
        let decided = if self.outside_repo_of_task(branch.task)?.is_some() {
            Err(delivery::OUTSIDE_KEPT.to_owned())
        } else {
            delivery::may_push(&passed)
        };
        let (status, why) = match decided {
            Ok(()) => (delivery::DUE, String::new()),
            Err(why) => (delivery::ACCEPTED, why),
        };
        branch.status = status.to_owned();
        self.store.save_branch(&branch)?;
        self.keep(branch.id, &why)?;
        Ok(())
    }

    /// An item in the loops ended blocked (the lock held): its newest submission, when its
    /// check passed, is kept on Manage's list with its bundle a download away (D22 of round 3:
    /// 949's work, which met the bar, survived only in the node's mirror). Nothing is pushed.
    pub(super) fn keep_blocked_submission(&self, task: crate::domain::TaskId) -> Done<()> {
        let Some(mut branch) = self.newest_branch(task)? else {
            return Ok(());
        };
        if branch.status != crate::domain::wing::SUBMITTED {
            return Ok(());
        }
        let passed = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == branch.id && c.status == "done")
            .max_by_key(|c| c.id)
            .is_some_and(|c| c.ok == Some(true));
        if !passed {
            return Ok(());
        }
        branch.status = delivery::KEPT_BLOCKED.to_owned();
        self.store.save_branch(&branch)?;
        self.keep(
            branch.id,
            "it passed its check; download its bundle, or move the item back to work",
        )
    }

    /// What the node is to do now.
    ///
    /// # Errors
    /// A store failure.
    pub fn deliveries(&self) -> Done<Deliveries> {
        let _held = self.held();
        let merging = self.merging()?;
        let mut due = Deliveries::default();
        for b in self.store.branches()? {
            match b.status.as_str() {
                delivery::DUE => due.push.push(Due {
                    branch: b,
                    scan: true,
                }),
                delivery::DUE_UNSCANNED => due.push.push(Due {
                    branch: b,
                    scan: false,
                }),
                delivery::ON_FORK if merging => due.merge.push(b),
                _ => {}
            }
        }
        Ok(due)
    }

    /// What the node says came of a branch it was asked about: kept, said in the thread, a
    /// notice that clears itself, and a card only when the owner is needed. Told once a step.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a branch that does not exist; [`Refusal::Finished`] for a
    /// step it is not waiting for.
    pub fn delivered(&self, branch: i64, report: &Delivered) -> Done<()> {
        let _held = self.held();
        let mut b = self
            .store
            .branches()?
            .into_iter()
            .find(|b| b.id == branch)
            .ok_or(EngineError::Unknown("branch"))?;
        let waits = match report {
            Delivered::Pushed { .. } => [delivery::DUE, delivery::DUE_UNSCANNED].as_slice(),
            Delivered::Held { .. } => [delivery::DUE].as_slice(),
            Delivered::Merged { .. } => [delivery::ON_FORK].as_slice(),
        };
        if !waits.contains(&b.status.as_str()) {
            return Err(EngineError::Refused(Refusal::Finished));
        }
        match report {
            Delivered::Held { found } => {
                b.status = delivery::HELD.to_owned();
                self.store.save_branch(&b)?;
                self.keep(b.id, &delivery::listed(found))?;
                let item = self.store.add_desk(
                    "privacy",
                    Some(b.task),
                    None,
                    Some(b.member),
                    &delivery::held_said(&b.name, found),
                    self.clock.now(),
                )?;
                self.emit(
                    EventKind::DeskAdded,
                    json!({ "item": item.0, "kind": "privacy", "task": b.task.0, "branch": b.name }),
                )?;
            }
            Delivered::Pushed { ok, text, needs } => {
                b.status = if *ok {
                    delivery::ON_FORK
                } else {
                    delivery::PUSH_FAILED
                }
                .to_owned();
                self.store.save_branch(&b)?;
                self.keep(b.id, if *ok { "" } else { text })?;
                self.said_on_the_record(&b, *ok, text)?;
                if *ok && !needs.is_empty() {
                    let item = self.store.add_desk(
                        "needs_owner",
                        Some(b.task),
                        None,
                        Some(b.member),
                        &delivery::needs_said(&b.name, needs),
                        self.clock.now(),
                    )?;
                    self.emit(
                        EventKind::DeskAdded,
                        json!({ "item": item.0, "kind": "needs_owner", "task": b.task.0 }),
                    )?;
                }
            }
            Delivered::Merged { ok, text } => {
                b.status = if *ok {
                    delivery::MERGED
                } else {
                    delivery::MERGE_FAILED
                }
                .to_owned();
                self.store.save_branch(&b)?;
                self.keep(b.id, if *ok { "" } else { text })?;
                // In develop now. No Deployed card follows: the check after a deploy retired
                // (ADR 0142), and the lab's recurring health check is the check.
                self.said_on_the_record(&b, *ok, text)?;
            }
        }
        Ok(())
    }

    /// A step the node took: a receipt on the task, a post in its thread, the feed, and a
    /// notice that clears itself.
    fn said_on_the_record(&self, b: &Branch, ok: bool, text: &str) -> Done<()> {
        let text = super::one_line(text, 1000);
        let receipt = self.store.add_call_receipt(&CallReceipt {
            id: 0,
            run: b.run,
            member: b.member,
            task: Some(b.task),
            tool: "push".to_owned(),
            args: format!("{}@{}", b.name, &b.head[..12.min(b.head.len())]),
            excerpt: text.clone(),
            ok,
            act: true,
            confidential: false,
            sandbox: false,
            at: self.clock.now(),
        })?;
        self.post_held_lock(
            Author::Endora,
            b.task,
            &format!("{text} (receipt r{receipt})"),
        )?;
        self.emit(
            EventKind::BranchPushed,
            json!({ "task": b.task.0, "branch": b.name, "head": b.head, "ok": ok,
                    "receipt": receipt, "member": b.member.0, "status": b.status }),
        )?;
        let item =
            self.store
                .add_desk("notice", Some(b.task), None, None, &text, self.clock.now())?;
        self.store.set_desk_status(item, "seen")?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "notice", "task": b.task.0 }),
        )?;
        Ok(())
    }

    /// The owner's word on a branch the privacy scan held (the lock held): pushed anyway, the
    /// scan no longer deciding; or sent back to the team to take out what was found.
    pub(super) fn decide_held(&self, found: &DeskItem, anyway: bool) -> Done<()> {
        let task = found.task.ok_or(EngineError::Unknown("task"))?;
        let mut b = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| b.task == task && b.status == delivery::HELD)
            .max_by_key(|b| b.id)
            .ok_or(EngineError::Unknown("held branch"))?;
        let listed = self.kept_of(b.id)?["why"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if anyway {
            b.status = delivery::DUE_UNSCANNED.to_owned();
            self.store.save_branch(&b)?;
            return self.post_held_lock(
                Author::Owner,
                task,
                &format!(
                    "The owner read what the privacy scan of {} found and said to push it anyway.",
                    b.name
                ),
            );
        }
        b.status = "sent_back".to_owned();
        self.store.save_branch(&b)?;
        let current = self.task(task)?;
        self.store.set_task_status(task, TaskStatus::Doing)?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "Not pushed: the privacy scan of {} found {listed}. The owner sent it back: take \
                 it out (the value is not said here), submit the branch again, then ask for \
                 review.",
                b.name
            ),
        )?;
        self.emit_status(&current, TaskStatus::Doing)?;
        if let Some(worker) = current.holder() {
            self.wake_held_lock(worker, Some(task), WakeReason::SentBack)?;
        }
        Ok(())
    }

    /// Manage's "Accepted, waiting to merge": each accepted branch not yet merged, newest
    /// first, with its task, when it was accepted and where it stands.
    pub(super) fn accepted_shown(&self) -> Done<Value> {
        let mut waiting: Vec<Branch> = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| delivery::waiting(&b.status))
            .collect();
        waiting.sort_by_key(|b| std::cmp::Reverse(b.id));
        let mut rows = Vec::new();
        for b in waiting {
            let kept = self.kept_of(b.id)?;
            let title = self
                .store
                .task(b.task)?
                .map(|t| t.title)
                .unwrap_or_default();
            rows.push(json!({
                "id": b.id, "task": b.task.0, "title": title, "branch": b.name,
                "status": b.status, "said": delivery::said(&b.status),
                "why": kept["why"], "at": kept["at"].as_i64().unwrap_or(b.at),
                "can_push": b.can_push,
            }));
        }
        Ok(Value::Array(rows))
    }
}
