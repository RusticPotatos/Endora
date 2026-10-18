//! The escalation ladder (ADR 0120, the owner's amendment of 2026-10-18): stuck work goes up
//! one rung at a time, each once, and reaches the owner's desk only as a board matter.
//!
//! Live, task #666 (a lead's plan inside its own approved project) went in circles: the lead
//! asked the CEO's leave, the tester asked another lead, the CEO was out of money, and
//! HQ's stranded-recovery finally put "Stuck… nobody came back to it" on the owner's desk.
//! Now: the area's lead decides; if it is still stuck, a huddle of the lead and up to four who
//! touched it gives one short view each, and the lead decides; only a decision that needs the
//! CEO goes to the CEO, and only a board matter to the board.
//!
//! Where a task is on the ladder, and its huddle, are kept in HQ's settings (`ladder:<task>`,
//! `huddle:<task>`); the huddle is written as a document on the task when the lead decides.

use serde_json::{Value, json};

use super::{Author, Done, Engine, EngineError, one_line};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::ladder::{self, Decision, Rung};
use crate::domain::{
    Document, EventKind, Label, Member, MemberId, Millis, Ref, Refusal, Role, RunId, Task, TaskId,
    TaskStatus, WakeReason,
};

/// Where HQ keeps where a task is on the ladder: `<rung>:<when>`.
const LADDER_KEY: &str = "ladder:";
/// Where HQ keeps a task's huddle, while it lasts.
const HUDDLE_KEY: &str = "huddle:";
/// The most characters a decision's words may run to.
const DECISION_MOST: usize = 1_000;
/// How many of a thread's posts the ladder's briefings show.
const HISTORY: usize = 8;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Where `task` is on the ladder, and since when.
    pub(super) fn rung_of(&self, task: TaskId) -> Done<Option<(Rung, Millis)>> {
        let Some(kept) = self.store.setting(&format!("{LADDER_KEY}{}", task.0))? else {
            return Ok(None);
        };
        let (rung, at) = kept.split_once(':').unwrap_or((kept.as_str(), "0"));
        Ok(Rung::from_name(rung).map(|r| (r, at.parse().unwrap_or(0))))
    }

    fn set_rung(&self, task: TaskId, rung: Rung) -> Done<()> {
        self.store.set_setting(
            &format!("{LADDER_KEY}{}", task.0),
            &format!("{}:{}", rung.name(), self.clock.now()),
        )?;
        Ok(())
    }

    fn huddle(&self, task: TaskId) -> Done<Option<Value>> {
        Ok(self
            .store
            .setting(&format!("{HUDDLE_KEY}{}", task.0))?
            .and_then(|kept| serde_json::from_str(&kept).ok()))
    }

    fn keep_huddle(&self, task: TaskId, huddle: &Value) -> Done<()> {
        self.store
            .set_setting(&format!("{HUDDLE_KEY}{}", task.0), &huddle.to_string())?;
        Ok(())
    }

    /// The last posts of a task's thread, for a briefing.
    fn history(&self, task: TaskId) -> Done<String> {
        let thread = self.store.thread(task)?;
        let mut told = String::from("Its history, newest last:\n");
        for p in thread.iter().rev().take(HISTORY).rev() {
            told.push_str(&format!(
                "- {}\n",
                Self::post_line(&p.author, &p.text, p.unsupported)
            ));
        }
        Ok(told)
    }

    /// Whether nothing (a turn queued, started or ended, or a wake) has happened on `task` for
    /// longer than `ms`.
    pub(super) fn idle_for(&self, task: TaskId, ms: Millis) -> Done<bool> {
        let runs = self.store.runs()?;
        let wakes = self.store.wakes()?;
        let last = runs
            .iter()
            .filter(|r| r.task == Some(task))
            .flat_map(|r| [Some(r.queued_at), r.started_at, r.ended_at])
            .flatten()
            .chain(wakes.iter().filter(|w| w.task == Some(task)).map(|w| w.at))
            .max()
            .unwrap_or(Millis::MIN);
        Ok(self.clock.now().saturating_sub(last) > ms)
    }

    /// Stuck work climbs one rung (see [`ladder::climb`]). Returns whether the ladder took
    /// it: `false` only when the task is in no lead's area (then the caller does as before).
    pub(super) fn climb_stuck(&self, task: &Task, why: &str) -> Done<bool> {
        let Some(lead) = self.area_lead(task)? else {
            return Ok(false);
        };
        // Only parked work climbs (ADR 0124): work held on purpose, or with a turn on it, waits
        // for what will bring it back, and what HQ cannot tell about is the lead's one look.
        let parked = self
            .diagnosis(task.id)?
            .is_some_and(|d| d.stopped == crate::domain::guards::Stopped::Parked);
        if !parked {
            return Ok(true);
        }
        let at = self.rung_of(task.id)?;
        let since = at.map_or(Millis::MIN, |(_, when)| when);
        let moved = self.store.runs()?.iter().any(|r| {
            r.task == Some(task.id)
                && r.started_at.is_some_and(|s| s >= since)
                && r.label == Some(Label::Advanced)
        });
        let Some(next) = ladder::climb(at.map(|(r, _)| r), moved) else {
            return Ok(true);
        };
        match next {
            Rung::Lead => self.ask_the_lead(task, &lead, why)?,
            Rung::Huddle => self.call_huddle(task, &lead, why)?,
            Rung::Ceo => self.to_the_ceo(task, &lead, "the lead did not decide it")?,
            Rung::Decided => {}
        }
        Ok(true)
    }

    /// The first rung: the area's lead decides.
    fn ask_the_lead(&self, task: &Task, lead: &Member, why: &str) -> Done<()> {
        self.set_rung(task.id, Rung::Lead)?;
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "Stuck: {why}. {}, this is in your area: decide it (decide_stuck: proceed, drop, \
                 hand_to, needs_ceo or needs_board), on evidence. It goes to the owner only if \
                 it is the board's.",
                lead.name
            ),
        )?;
        self.emit(
            EventKind::StuckClimbed,
            json!({ "task": task.id.0, "rung": Rung::Lead.name(), "to": lead.id.0, "why": why }),
        )?;
        self.wake_held_lock(lead.id, Some(task.id), WakeReason::Decide)?;
        Ok(())
    }

    /// The second rung: a huddle. The lead and up to four members who touched the task in the
    /// last two days; each of those gives one short view in one turn, then the lead decides.
    pub(super) fn call_huddle(&self, task: &Task, lead: &Member, why: &str) -> Done<()> {
        let members = self.store.members()?;
        let now = self.clock.now();
        let mut touched: Vec<(MemberId, Millis)> = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.task == Some(task.id))
            .filter_map(|r| r.started_at.map(|at| (r.member, at)))
            .collect();
        for post in self.store.thread(task.id)? {
            if let Some(m) = members.iter().find(|m| m.name == post.author) {
                touched.push((m.id, post.at));
            }
        }
        let left_out: Vec<MemberId> = members
            .iter()
            .filter(|m| m.role == Role::Ceo || !m.at_work())
            .map(|m| m.id)
            .collect();
        let heard = ladder::attendees(&touched, lead.id, &left_out, now);
        self.set_rung(task.id, Rung::Huddle)?;
        let names: Vec<String> = heard
            .iter()
            .filter_map(|id| members.iter().find(|m| m.id == *id))
            .map(|m| m.name.clone())
            .collect();
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "Huddle: {why}. {} and {}: each gives a view in three sentences at most, with \
                 evidence (huddle_view); then {} decides.",
                lead.name,
                if names.is_empty() {
                    "nobody else".to_owned()
                } else {
                    names.join(", ")
                },
                lead.name
            ),
        )?;
        let mut all = vec![lead.id.0];
        all.extend(heard.iter().map(|m| m.0));
        self.emit(
            EventKind::HuddleCalled,
            json!({ "task": task.id.0, "lead": lead.id.0, "members": all, "why": why }),
        )?;
        let mut done = Vec::new();
        for member in &heard {
            let receipt = self.wake_held_lock(*member, Some(task.id), WakeReason::Huddle)?;
            if matches!(receipt, crate::domain::Receipt::Refused(_)) {
                done.push(member.0);
            }
        }
        let huddle = json!({ "lead": lead.id.0, "members": heard.iter().map(|m| m.0).collect::<Vec<_>>(),
                             "at": now, "why": why, "views": [], "done": done, "deciding": false });
        self.keep_huddle(task.id, &huddle)?;
        self.huddle_heard(task.id)
    }

    /// When everyone in a huddle has been heard (or could not come), its lead decides.
    fn huddle_heard(&self, task: TaskId) -> Done<()> {
        let Some(mut huddle) = self.huddle(task)? else {
            return Ok(());
        };
        if huddle["deciding"] == true {
            return Ok(());
        }
        let ids = |key: &str| -> Vec<i64> {
            huddle[key]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default()
        };
        let (members, done) = (ids("members"), ids("done"));
        if !members.iter().all(|m| done.contains(m)) {
            return Ok(());
        }
        self.huddle_to_decide(task, &mut huddle)
    }

    fn huddle_to_decide(&self, task: TaskId, huddle: &mut Value) -> Done<()> {
        huddle["deciding"] = json!(true);
        self.keep_huddle(task, huddle)?;
        let lead = MemberId(huddle["lead"].as_i64().unwrap_or(0));
        self.wake_held_lock(lead, Some(task), WakeReason::Decide)?;
        Ok(())
    }

    /// A huddle whose views did not all come within [`ladder::VIEWS_WAIT_MS`] goes to its lead
    /// to decide on what was said. On the tick.
    pub(super) fn huddles_due(&self) -> Done<()> {
        let now = self.clock.now();
        for task in self.store.tasks()? {
            if task.status.finished() {
                continue;
            }
            let Some(mut huddle) = self.huddle(task.id)? else {
                continue;
            };
            let at = huddle["at"].as_i64().unwrap_or(now);
            if huddle["deciding"] != true && now - at >= ladder::VIEWS_WAIT_MS {
                self.huddle_to_decide(task.id, &mut huddle)?;
            }
        }
        Ok(())
    }

    /// A member's turn that a huddle woke ended: it was heard, whether it gave a view or not.
    /// A huddle's wake that folded into a turn already running (the member was mid-turn on the
    /// task when the huddle was called) asks again, now, for a turn of its own.
    pub(super) fn after_huddle_turn(&self, run: &Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        if self.reason_for(run)? != WakeReason::Huddle.name() {
            let folded = self.store.wakes()?.iter().any(|w| {
                w.run == Some(run.id)
                    && w.id != run.wake
                    && w.member == run.member
                    && w.reason == WakeReason::Huddle
            });
            if folded && self.huddle(task)?.is_some_and(|h| h["deciding"] != true) {
                self.wake_held_lock(run.member, Some(task), WakeReason::Huddle)?;
            }
            return Ok(());
        }
        let Some(mut huddle) = self.huddle(task)? else {
            return Ok(());
        };
        if let Some(done) = huddle["done"].as_array_mut() {
            if !done.iter().any(|d| d.as_i64() == Some(run.member.0)) {
                done.push(json!(run.member.0));
            }
        }
        self.keep_huddle(task, &huddle)?;
        self.huddle_heard(task)
    }

    /// A member's view for a huddle, in its one short turn: three sentences at most, on
    /// evidence. Its turn then ends.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NotInTheHuddle`]; what [`ladder::view`] refuses; what
    /// the evidence rule refuses.
    pub fn huddle_view_in(&self, run: RunId, task: TaskId, view: &str, cited: &[Ref]) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let Some(mut huddle) = self.huddle(task)? else {
            return Err(EngineError::Refused(Refusal::NotInTheHuddle));
        };
        let me = turn.member.0;
        let asked = huddle["members"]
            .as_array()
            .is_some_and(|m| m.iter().any(|x| x.as_i64() == Some(me)));
        let gave = huddle["views"]
            .as_array()
            .is_some_and(|v| v.iter().any(|x| x["member"].as_i64() == Some(me)));
        if !asked || gave || huddle["deciding"] == true {
            return Err(EngineError::Refused(Refusal::NotInTheHuddle));
        }
        let view = ladder::view(view).map_err(EngineError::Refused)?;
        self.judge_decision(cited, turn.id)?;
        let said = self.evidence_said(cited)?;
        let member = self.member(turn.member)?;
        if let Some(views) = huddle["views"].as_array_mut() {
            views
                .push(json!({ "member": me, "name": member.name, "view": view, "evidence": said }));
        }
        if let Some(done) = huddle["done"].as_array_mut() {
            done.push(json!(me));
        }
        self.keep_huddle(task, &huddle)?;
        self.post_held_lock_marked(
            Author::Member(member.id),
            task,
            &format!("My view for the huddle: {view}"),
            cited,
            false,
        )?;
        self.huddle_heard(task)
    }

    /// The third rung: the CEO, who directs the lead, decides a proposal, or puts a board
    /// matter to the board. With no CEO at work, the owner's desk, as before there was one.
    fn to_the_ceo(&self, task: &Task, lead: &Member, why: &str) -> Done<()> {
        self.set_rung(task.id, Rung::Ceo)?;
        let ceo = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.role == Role::Ceo && m.at_work());
        self.emit(
            EventKind::StuckClimbed,
            json!({ "task": task.id.0, "rung": Rung::Ceo.name(),
                    "to": ceo.as_ref().map(|c| c.id.0), "why": why }),
        )?;
        let Some(ceo) = ceo else {
            return self.stuck_on_the_desk(task, &format!("{}: {why}", lead.name));
        };
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!("This goes to {}, from {}: {why}", ceo.name, lead.name),
        )?;
        self.wake_held_lock(ceo.id, Some(task.id), WakeReason::Decide)?;
        Ok(())
    }

    /// Stuck work on the owner's desk: only when no rung is left to take it, and once while it
    /// waits there (ADR 0124: it closes itself when the work moves again).
    pub(super) fn stuck_on_the_desk(&self, task: &Task, why: &str) -> Done<()> {
        let there = self
            .store
            .desk()?
            .iter()
            .any(|d| d.kind == "stuck" && d.status == "waiting" && d.task == Some(task.id));
        if there {
            return Ok(());
        }
        let item = self.store.add_desk(
            "stuck",
            Some(task.id),
            None,
            None,
            &format!("Stuck: {}. {why}", task.title),
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "stuck", "task": task.id.0 }),
        )
    }

    /// The lead of a stuck task's area decides it, on evidence, on its own or after its
    /// huddle: proceed (as it says), drop, hand it to a member of its team, needs the CEO, or
    /// needs the board. A huddle is written as a document on the task.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NotStuckForYou`]; [`EngineError::Unknown`] for no
    /// words, or `hand_to` with nobody; [`Refusal::NotBelow`] for a member outside the lead's
    /// team; what the evidence rule refuses;
    /// [`crate::domain::world::WorldRefusal::HandoverNeeded`] for work handed on or dropped
    /// without its handover note (ADR 0122).
    #[allow(clippy::too_many_arguments)]
    pub fn decide_stuck_in(
        &self,
        run: RunId,
        task: TaskId,
        decision: Decision,
        detail: &str,
        to: Option<MemberId>,
        cited: &[Ref],
        handover: Option<&crate::domain::world::HandoverNote>,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let current = self.task(task)?;
        let lead = self.member(turn.member)?;
        let ours = self.area_lead(&current)?.is_some_and(|l| l.id == lead.id);
        let on_it = matches!(self.rung_of(task)?, Some((Rung::Lead | Rung::Huddle, _)));
        if !ours || !on_it || current.status.finished() {
            return Err(EngineError::Refused(Refusal::NotStuckForYou));
        }
        let detail = one_line(detail, DECISION_MOST);
        if detail.is_empty() {
            return Err(EngineError::Unknown("words for the decision"));
        }
        let hands_on = matches!(decision, Decision::HandTo | Decision::Drop);
        let handover = match handover {
            Some(note) => Some(note),
            None if hands_on => {
                return Err(EngineError::Refused(Refusal::World(
                    crate::domain::world::WorldRefusal::HandoverNeeded,
                )));
            }
            None => None,
        };
        self.judge_decision(cited, turn.id)?;
        let evidence = self.evidence_said(cited)?;
        let members = self.store.members()?;
        let target = match (decision, to) {
            (Decision::HandTo, None) => return Err(EngineError::Unknown("member to hand it to")),
            (Decision::HandTo, Some(to)) => {
                let found = self.member(to)?;
                let in_team = found.id == lead.id
                    || crate::domain::staffing::lead_of(&found, &members) == Some(lead.id);
                if !in_team || !found.at_work() {
                    return Err(EngineError::Refused(Refusal::NotBelow));
                }
                Some(found)
            }
            _ => None,
        };
        let huddle = self.huddle(task)?;
        let said = format!(
            "Decided by {}: {}: {detail} (evidence: {evidence})",
            lead.name,
            decision.name().replace('_', " ")
        );
        self.write_huddle(&current, huddle.as_ref(), &said, turn.id)?;
        self.post_held_lock(Author::Member(lead.id), task, &said)?;
        self.store
            .set_setting(&format!("{HUDDLE_KEY}{}", task.0), "")?;
        self.set_rung(task, Rung::Decided)?;
        self.mark_progress(lead.id, task)?;
        let heard: Vec<i64> = huddle
            .as_ref()
            .and_then(|h| h["members"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_i64)
            .collect();
        self.emit(
            EventKind::HuddleDecided,
            json!({ "task": task.0, "lead": lead.id.0, "decision": decision.name(),
                    "members": heard, "huddle": huddle.is_some() }),
        )?;
        if let (true, Some(note)) = (hands_on, handover) {
            self.keep_handover(
                &turn,
                task,
                current.owner.or(current.assignee),
                target.as_ref().map(|t| t.id),
                note,
            )?;
        }
        match decision {
            Decision::Proceed => self.back_to_work(&current, current.assignee, &lead),
            Decision::HandTo => self.back_to_work(&current, target.map(|t| t.id), &lead),
            Decision::Drop => {
                self.store.set_task_status(task, TaskStatus::Dropped)?;
                self.emit_status(&current, TaskStatus::Dropped)?;
                self.after_finishing(&current)
            }
            Decision::NeedsCeo => self.to_the_ceo(&current, &lead, &detail),
            Decision::NeedsBoard => self.to_the_ceo(
                &current,
                &lead,
                &format!(
                    "the lead says this is the board's: {detail}. Put it to the board with \
                     ask_owner, as a decision, if it is"
                ),
            ),
        }
    }

    /// Decided work goes on: `todo` for `to` (or whoever it was for), nudged afresh, and woken
    /// unless that is the lead deciding, whose turn goes on.
    fn back_to_work(&self, task: &Task, to: Option<MemberId>, lead: &Member) -> Done<()> {
        self.store.release(task.id)?;
        if to.is_some() {
            self.store.set_assignee(task.id, to)?;
        }
        if task.status != TaskStatus::Todo {
            self.store.set_task_status(task.id, TaskStatus::Todo)?;
            self.emit_status(task, TaskStatus::Todo)?;
        }
        self.store.set_nudged(task.id, false)?;
        let Some(to) = to.filter(|t| *t != lead.id) else {
            return Ok(());
        };
        self.wake_held_lock(to, Some(task.id), WakeReason::Assigned)?;
        Ok(())
    }

    /// The huddle as a document on the task: who said what, and what the lead decided.
    fn write_huddle(
        &self,
        task: &Task,
        huddle: Option<&Value>,
        decided: &str,
        run: RunId,
    ) -> Done<()> {
        let Some(huddle) = huddle else {
            return Ok(());
        };
        let mut text = format!(
            "Huddle on #{} {}\nWhy: {}\n\n",
            task.id,
            task.title,
            huddle["why"].as_str().unwrap_or("")
        );
        let views = huddle["views"].as_array().cloned().unwrap_or_default();
        if views.is_empty() {
            text.push_str("Nobody gave a view.\n");
        }
        for v in views {
            text.push_str(&format!(
                "- {}: {} (evidence: {})\n",
                v["name"].as_str().unwrap_or("?"),
                v["view"].as_str().unwrap_or(""),
                v["evidence"].as_str().unwrap_or("")
            ));
        }
        text.push_str(&format!("\n{decided}\n"));
        self.store.add_document(&Document {
            id: 0,
            task: task.id,
            run: Some(run),
            title: format!("Huddle on #{}", task.id),
            text,
            at: self.clock.now(),
        })?;
        Ok(())
    }

    /// What a turn the ladder woke is told: the huddle's question, or the decision asked of
    /// the lead (with the huddle's views) or of the CEO. Empty for any other turn.
    pub(super) fn ladder_said(&self, member: &Member, run: &Run, task: &Task) -> Done<String> {
        let reason = self.reason_for(run)?;
        if reason == WakeReason::Huddle.name() {
            return Ok(format!(
                "A huddle on this task, which is stuck: give your view in three sentences at \
                 most, with evidence (huddle_view: task, view, evidence). It is one short turn: \
                 give it and stop. {} decides.\n{}",
                self.huddle(task.id)?
                    .and_then(|h| h["lead"].as_i64())
                    .and_then(|id| self.member(MemberId(id)).ok())
                    .map_or_else(|| "The lead".to_owned(), |m| m.name),
                self.history(task.id)?
            ));
        }
        if reason != WakeReason::Decide.name() {
            return Ok(String::new());
        }
        if member.role == Role::Ceo {
            return Ok(format!(
                "Stuck work reached you up the ladder (its area's lead first, then a huddle). \
                 Direct the lead (direct), decide a proposal, or put a board matter to the \
                 board (ask_owner).\n{}",
                self.history(task.id)?
            ));
        }
        let mut told = String::from(
            "This is stuck in your area, and it is yours to decide (decide_stuck: proceed with \
             what, drop, hand_to a member of your team, needs_ceo for money, a new goal or \
             another area, needs_board for the owner alone), on evidence. Inside an approved \
             project you need nobody's leave.\n",
        );
        if let Some(huddle) = self.huddle(task.id)? {
            told.push_str("Your huddle's views:\n");
            let views = huddle["views"].as_array().cloned().unwrap_or_default();
            if views.is_empty() {
                told.push_str("- nobody gave a view\n");
            }
            for v in views {
                told.push_str(&format!(
                    "- {}: {} (evidence: {})\n",
                    v["name"].as_str().unwrap_or("?"),
                    v["view"].as_str().unwrap_or(""),
                    v["evidence"].as_str().unwrap_or("")
                ));
            }
        }
        told.push_str(&self.history(task.id)?);
        Ok(told)
    }
}
