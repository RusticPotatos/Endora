//! Turns: queued, started, ended and retried.

use super::{COLLEAGUE_HELP, Done, Engine, EngineError, STANDARDS};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::effort::{self, Effort};
use crate::domain::mac;
use crate::domain::review;
use crate::domain::runs;
use crate::domain::{
    Brain, CheckBackStatus, EventKind, Fault, Member, Millis, Receipt, RunStatus, WakeReason,
};
use serde_json::json;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// How hard the member's Claude tries this turn (see [`effort::effort_for`]).
    pub(super) fn effort_of(&self, member: &Member) -> Option<Effort> {
        effort::effort_for(member.brain, member.role, effort::Weight::Ordinary)
    }

    pub(super) fn reason_for(&self, run: &Run) -> Done<&'static str> {
        Ok(self.store.wake(run.wake)?.map_or("", |w| w.reason.name()))
    }

    /// Ends a turn: settles its money, closes its wake, and lets the next held wake on its task
    /// go. A turn that moved its task forward earns the task a fresh recovery nudge.
    pub(super) fn end(&self, mut run: Run, status: RunStatus, why: &str) -> Done<()> {
        let now = self.clock.now();
        if run.started_at.is_some() {
            self.store.settle(run.id, run.member, self.today(), now)?;
        }
        let label = self.settle_label(&mut run)?;
        run.status = status;
        run.ended_at = Some(now);
        run.end_reason = why.to_owned();
        run.reserved = 0;
        self.store.save_run(&run)?;
        self.store
            .set_wake(run.wake, Receipt::Finished, Some(run.id))?;
        // The tokens as the relay reported them, which only it adds to: all the input sent
        // and the output, for a turn that sent any, and the share the cache gave.
        let kept = self.store.run(run.id)?;
        let cached = kept.as_ref().and_then(|r| {
            runs::cached_share(r.input_tokens, r.cache_read_tokens, r.cache_write_tokens)
        });
        let tokens = kept
            .as_ref()
            .map(|r| {
                (
                    r.input_tokens + r.cache_read_tokens + r.cache_write_tokens,
                    r.output_tokens,
                )
            })
            .filter(|(input, output)| input + output > 0);
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": status.name(), "why": why, "moved_forward": run.moved_forward,
                    "label": label.map(crate::domain::Label::name),
                    "spent": run.spent, "cached": cached,
                    "input": tokens.map(|t| t.0), "output": tokens.map(|t| t.1) }),
        )?;
        if let Some(task) = run.task {
            // A turn that showed something earns its task a fresh recovery nudge; a reading the
            // turn before had already taken shows nothing, so it earns none.
            let repeated = self
                .shown_nothing(run.member, task)?
                .last()
                .is_some_and(|(id, nothing)| *id == run.id && *nothing);
            if label.is_some_and(crate::domain::Label::productive) && !repeated {
                self.store.set_nudged(task, false)?;
            }
            self.release_held(task)?;
        }
        if status == RunStatus::Succeeded && label.is_some() {
            self.after_nothing_shown(&run)?;
        }
        // A deploy check's one turn is over: its verdict, once (ADR 0115, amended).
        self.after_a_deploy_check(&run)?;
        Ok(())
    }

    /// Ends a turn that put a blocker to someone: a question, a draft, a decision, an approval.
    pub(super) fn end_blocked(&self, mut run: Run, status: RunStatus, why: &str) -> Done<()> {
        run.blocked = true;
        self.end(run, status, why)
    }

    /// A failed turn: tried again a minute later, at most twice; then failed for good. A turn the
    /// platform cut short (HQ or the runner restarted under it, its container vanished) did
    /// nothing wrong: the first time, its start does not count against its retries, and it says
    /// it was interrupted (ADR 0124, amended).
    pub(super) fn fail(&self, mut run: Run, why: &str, fault: Fault) -> Done<bool> {
        let now = self.clock.now();
        let why = match fault {
            Fault::Worker => why.to_owned(),
            Fault::Platform => {
                run.interrupted = run.interrupted.saturating_add(1);
                format!("interrupted: {why}")
            }
        };
        let why = why.as_str();
        let assistance_delivery = false;
        // A turn on the owner's Mac that failed because the Mac went away (the owner came back,
        // the laptop left its charger) did nothing wrong: it is tried again when the Mac is
        // free, and the failure is not counted against it (ADR 0130).
        let on_the_mac = run.brain == Some(Brain::Mac);
        if !assistance_delivery
            && on_the_mac
            && mac::wait_for(self.mac_facts().as_ref(), now, false).is_some()
        {
            run.attempts = run.attempts.saturating_sub(1);
        }
        let own = if assistance_delivery {
            run.attempts
        } else {
            runs::own_attempts(run.attempts, run.interrupted)
        };
        let Some(due) = runs::retry_after_failure(own, now) else {
            self.end(run, RunStatus::Failed, why)?;
            return Ok(false);
        };
        // A retry is a stale continuation: it backs off like one.
        let due = due.max(now + self.back_off(run.member, run.task, WakeReason::Retry)?);
        if run.started_at.is_some() {
            self.store.settle(run.id, run.member, self.today(), now)?;
        }
        run.status = RunStatus::RetryWaiting;
        run.retry_at = Some(due);
        run.reserved = 0;
        run.end_reason = why.to_owned();
        self.store.save_run(&run)?;
        self.emit(
            EventKind::RunEnded,
            json!({ "run": run.id.0, "member": run.member.0, "task": run.task.map(|t| t.0),
                    "result": RunStatus::RetryWaiting.name(), "why": why,
                    "interrupted": fault == Fault::Platform }),
        )?;
        Ok(true)
    }

    /// What a member is told at the start of a turn: who it is, then what of the team's world
    /// its situation needs within its budget (ADR 0122), then its work; and at the very end,
    /// what of the world was left out and how to read it. The run records its situation.
    pub(super) fn briefing(&self, member: &Member, run: &Run) -> Done<String> {
        let (_, memory) = self.memory(member, run)?;
        let mut told = self.briefing_body(member, run, &memory.text)?;
        told.push_str(&memory.omitted);
        // In the loops: the map and what the loop did lately (ADR 0142). Never in a game: what
        // other turns in a league said is the other side's thinking.
        let in_a_game = match run.task {
            Some(task) => self.in_a_scenario(task)?,
            None => false,
        };
        if self.in_the_loops(member.id)? && !in_a_game {
            told.push_str(&self.loops_said(run)?);
            // Its own section: the item's handover, when it came to this member (ADR 0142,
            // amended 2026-10-30).
            told.push_str(&self.handover_briefed(member, run)?);
        }
        // Decided last, once the briefing has recorded what it showed the turn: a turn is told
        // of colleague help only when it is offered the tools (downtime lists them itself).
        if self.reason_for(run)? != WakeReason::Downtime.name()
            && self.colleague_help_said(member, run)?
        {
            told.push_str(COLLEAGUE_HELP);
        }
        Ok(told)
    }

    pub(super) fn colleague_help_said(&self, _member: &Member, _run: &Run) -> Done<bool> {
        // Colleagues are no longer asked (ADR 0142): the loops' checks and ranks are the help.
        Ok(false)
    }

    /// The briefing, around what it carries of the world (`memory`). Every member is told the
    /// same way (ADR 0142): no roles, only its model's rank.
    pub(super) fn briefing_body(&self, member: &Member, run: &Run, memory: &str) -> Done<String> {
        let mut told = format!(
            "You are {}, rank {} of {}, thinking with {}. Today is {}.\n",
            member.name,
            member.brain.rank(),
            crate::domain::Brain::BOTTOM,
            member.brain.label(),
            self.said_today()
        );
        // What it knows, first: its situation's pieces of the world (ADR 0118, 0122).
        told.push_str(memory);
        // Downtime has no task to brief: a chess move, or nothing (ADR 0119).
        if self.reason_for(run)? == WakeReason::Downtime.name() {
            told.push_str(&self.downtime_said(member, run.id)?);
            told.push_str("Your tools at HQ: my_turn, chess, read_doc, read_world.\n");
            return Ok(told);
        }
        told.push_str(STANDARDS);
        told.push_str(self.wing_rules(member)?);
        let Some(task) = run.task else {
            told.push_str("This turn is about no task: look over the team's work.\n");
            return Ok(told);
        };
        // Each section is classified by the lineage of what it shows, and what has none fails
        // closed (ADR 0138 A2c): the task's body and criteria, its parents and goal, and the
        // thread's posts by their recorded origins.
        let input = self
            .store
            .task_inputs(task)?
            .ok_or(EngineError::Unknown("task"))?;
        self.inherit_authored_input(run.id, input.body_origin)?;
        if !input.task.criteria.is_empty() {
            self.inherit_authored_input(run.id, input.criteria_origin)?;
        }
        let task = input.task;
        told.push_str(&format!(
            "Your task: {} (#{}, {}, {})\n{}\n",
            task.title,
            task.id,
            task.status.name(),
            task.priority.name(),
            task.description
        ));
        told.push_str(&self.criteria_said(&task));
        if let Some(nudge) = self.nudge_for(member.id, run)? {
            told.push_str(&format!("{nudge}\n"));
        }
        let note = self.goal_note_briefed(task.goal)?;
        if !note.is_empty() {
            self.unclassified_message_input(run.id)?;
            told.push_str(&format!("Notes from the owner for this goal: {note}\n"));
        }
        told.push_str(&self.why_said(run, &task)?);
        let documents = self.documents_said(task.id)?;
        if !documents.is_empty() {
            self.documents_reached(run.id, task.id)?;
        }
        told.push_str(&documents);
        if !task.waits_on.is_empty() {
            let waits: Vec<String> = task.waits_on.iter().map(|t| format!("#{t}")).collect();
            told.push_str(&format!("It waits on: {}\n", waits.join(", ")));
        }
        // The thread with each post's recorded origin, read together.
        let thread = self.all_authored_posts(task.id)?;
        told.push_str(&self.news_said(member, run, &task, &thread)?);
        if self.reason_for(run)? == WakeReason::ReviewRequested.name() {
            told.push_str(&self.review_asked_said(run, task.id, &thread)?);
        }
        // Checks' results have no lineage yet: shown, they fail closed.
        let untraced = self.checking_said(member, run, &task)?;
        if !untraced.is_empty() {
            self.unclassified_message_input(run.id)?;
        }
        told.push_str(&untraced);
        // HQ's own tools, never as all a member has: live, a lead on Claude offered the home
        // lab's read took this line for its whole kit and said it could not read the lab.
        let names: Vec<String> = crate::application::protocol::tools()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .collect();
        told.push_str(&format!(
            "Your tools at HQ: {}. Endora's own tools on the home lab (homelab_status and the \
             like) come beside them: what your tool list offers, you have.\n",
            names.join(", ")
        ));
        Ok(told)
    }

    /// Why the task exists: its title, its parents' and its goal's, each read with its lineage.
    fn why_said(&self, run: &Run, task: &crate::domain::Task) -> Done<String> {
        let mut why = vec![task.title.clone()];
        let mut parent = task.parent;
        while let Some(id) = parent {
            let up = self
                .store
                .task_inputs(id)?
                .ok_or(EngineError::Unknown("parent task"))?;
            self.inherit_authored_input(run.id, up.body_origin)?;
            why.push(up.task.title.clone());
            parent = up.task.parent;
        }
        if let Some(goal) = self.store.goal_input(task.goal)? {
            self.inherit_authored_input(run.id, goal.origin)?;
            why.push(format!("goal: {}", goal.goal.title));
        }
        Ok(format!("Why it exists: {}\n", why.join(" ← ")))
    }

    /// What came since the member's last turn on the task: the check-backs it set that fired,
    /// and the thread's new posts, each read with its lineage.
    fn news_said(
        &self,
        member: &Member,
        run: &Run,
        task: &crate::domain::Task,
        thread: &[crate::application::history_ports::PostInput],
    ) -> Done<String> {
        let mut said = String::new();
        let since = self
            .store
            .runs_of(task.id)?
            .iter()
            .filter(|r| r.member == member.id && r.id != run.id)
            .filter_map(|r| r.started_at)
            .max()
            .unwrap_or(Millis::MIN);
        for came_back in self.store.check_backs()?.iter().filter(|c| {
            c.member == member.id
                && c.task == task.id
                && c.status == CheckBackStatus::Fired
                && c.done_at.is_some_and(|at| at > since)
        }) {
            self.unclassified_message_input(run.id)?;
            said.push_str(&format!(
                "You said you would check back: {}\n",
                came_back.why
            ));
        }
        let members = self.store.members()?;
        // The handover this briefing carries is told in its own section, not again here.
        let handover = self.handover_kept_for(member.id, task.id)?;
        let mut new = Vec::new();
        for input in thread
            .iter()
            .filter(|p| p.post.at > since)
            .filter(|p| handover.as_deref() != Some(p.post.text.as_str()))
        {
            self.inherit_authored_input(run.id, input.origin)?;
            let p = &input.post;
            new.push(Self::post_line(&members, &p.author, &p.text, p.unsupported));
        }
        if new.is_empty() {
            said.push_str("Nothing new in the thread since your last turn.\n");
        } else {
            said.push_str("New in the thread since your last turn:\n");
            for line in new {
                said.push_str(&format!("- {line}\n"));
            }
        }
        Ok(said)
    }

    /// Who asks for this review, and how a review is done; the whole thread is read for it.
    fn review_asked_said(
        &self,
        run: &Run,
        task: crate::domain::TaskId,
        thread: &[crate::application::history_ports::PostInput],
    ) -> Done<String> {
        for input in thread {
            self.inherit_authored_input(run.id, input.origin)?;
        }
        let asked = self.store.thread(task)?.into_iter().rev().find_map(|p| {
            p.text.contains(" → in_review").then(|| {
                format!(
                    "{} asks for your review.\n{}",
                    p.author,
                    review::REVIEW_WORK
                )
            })
        });
        Ok(asked
            .map(|asked| format!("{}\n", asked.trim_end()))
            .unwrap_or_default())
    }
}
