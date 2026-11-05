//! Turns: queued, started, ended and retried.

use super::{Done, Engine, EngineError, STANDARDS};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::handover;
use crate::domain::mac;
use crate::domain::review;
use crate::domain::runs;
use crate::domain::{
    Brain, CheckBackStatus, EventKind, Fault, Member, Millis, Receipt, RunStatus, TaskId,
    WakeReason,
};

/// The most of the owner's posts on an item a reviewer is told, newest last, and the most of
/// each, in characters.
const OWNER_POSTS_MOST: usize = 3;
const OWNER_LINE_MOST: usize = 300;
use serde_json::json;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
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
        // A turn that never started ran nothing: it has no label, so it shows nothing for or
        // against its member and backs off nothing (ADR 0142, amended 2026-10-31).
        let label = if runs::is_never_started(why) {
            run.label = None;
            None
        } else {
            self.settle_label(&mut run)?
        };
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
        // A turn the worker ended at its prompt's budget goes on fresh (ADR 0142, amended
        // 2026-10-30).
        if status == RunStatus::Succeeded {
            self.after_out_of_room(&run)?;
            // A review turn that ended with neither verdict (D26).
            self.after_review_turn(&run)?;
        }
        Ok(())
    }

    /// D2 of round 2 (ADR 0142, amended 2026-10-31): open work whose last turn never started (the
    /// runner could not start the member's runtime) is woken again for that member, as it was
    /// woken before, once [`runs::wake_again_after`] has passed since that turn ended. A turn
    /// that never started is no failure of the work, so the item neither counts one nor climbs:
    /// only a failed check or a send-back moves an item up a rank. Work in review is woken again
    /// for its reviewer, other work for its holder; anything else has moved on.
    pub(super) fn wake_after_failed_starts(
        &self,
        tasks: &[crate::domain::Task],
        live: &[Option<TaskId>],
        now: Millis,
    ) -> Done<()> {
        let members = self.store.members()?;
        for task in tasks {
            if live.contains(&Some(task.id)) || !self.store.held_wakes(task.id)?.is_empty() {
                continue;
            }
            let mut ended: Vec<Run> = self
                .store
                .runs_of(task.id)?
                .into_iter()
                .filter(|r| !r.status.live())
                .collect();
            ended.sort_by_key(|r| std::cmp::Reverse(r.id));
            let Some(last) = ended.first() else {
                continue;
            };
            let seen: Vec<runs::EndedTurn> = ended
                .iter()
                .map(|r| runs::EndedTurn {
                    member: r.member,
                    never_started: r.status == RunStatus::Failed
                        && runs::is_never_started(&r.end_reason),
                    ended_at: r.ended_at,
                })
                .collect();
            let at_work = members.iter().any(|m| m.id == last.member && m.at_work());
            if !runs::woken_again_after_failed_starts(
                &seen,
                task.status,
                task.holder(),
                at_work,
                now,
            ) || self.in_a_scenario(task.id)?
            {
                continue;
            }
            let reason = self
                .store
                .wake(last.wake)?
                .map_or(WakeReason::Retry, |w| w.reason);
            self.wake_held_lock(last.member, Some(task.id), reason)?;
        }
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
        // A review is told what the review needs, not the world or the thread's history (ADR
        // 0142, amended 2026-10-30).
        if let Some(task) = run.task {
            if self.reason_for(run)? == WakeReason::ReviewRequested.name() {
                return self.review_briefing(member, run, task);
            }
        }
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
            told.push_str(&self.alone_said(member, run)?);
            // Its own section: the item's handover, when it came to this member (ADR 0142,
            // amended 2026-10-30).
            told.push_str(&self.handover_briefed(member, run)?);
        }
        Ok(told)
    }

    /// The briefing, around what it carries of the world (`memory`). Every member is told the
    /// same way (ADR 0142): no roles, only its model's rank.
    pub(super) fn briefing_body(&self, member: &Member, run: &Run, memory: &str) -> Done<String> {
        let who = Self::who_said(member, &self.said_today());
        // Downtime has no task to brief: a chess move, or nothing (ADR 0119), and no map of the
        // world either (30 Oct: a move needs the board, not the house).
        if self.reason_for(run)? == WakeReason::Downtime.name() {
            let mut told = who;
            told.push_str(&self.downtime_said(member, run.id)?);
            told.push_str("Your tools at HQ: my_turn, chess.\n");
            return Ok(told);
        }
        // What every work turn is told alike comes first, then who the member is and the rest:
        // a provider's prompt cache reads the same opening for every turn (ADR 0142, amended
        // 2026-10-31, after Ralph: the same prompt every pass, what changes at its end).
        let mut told = Self::opening_said();
        told.push_str(&who);
        // What it knows, first: its situation's pieces of the world (ADR 0118, 0122).
        told.push_str(memory);
        told.push_str(self.wing_rules(member, run.task)?);
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
        // The loop's lessons, right after the item (ADR 0142, amended 2026-10-31).
        told.push_str(&self.lessons_briefed(member, run, &task)?);
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
        told.push_str(&self.submission_said(run, task.id)?);
        // Checks' results have no lineage yet: shown, they fail closed.
        let untraced = self.checking_said(member, run, &task)?;
        if !untraced.is_empty() {
            self.unclassified_message_input(run.id)?;
        }
        told.push_str(&untraced);
        Ok(told)
    }

    /// Who the member is, its rank and model, and the day: the first line of a briefing that
    /// varies from turn to turn.
    fn who_said(member: &Member, today: &str) -> String {
        format!(
            "You are {}, rank {} of {}, thinking with {}. Today is {today}.\n",
            member.name,
            member.brain.rank(),
            crate::domain::Brain::BOTTOM,
            member.brain.label(),
        )
    }

    /// The opening of every work and review turn's briefing, the same bytes in each: HQ's tools,
    /// the team's standards and how to leave the loop a lesson. Nothing in it may vary by
    /// member, item or time (`prefix_tests` in the worker holds it to that).
    fn opening_said() -> String {
        format!(
            "{}{STANDARDS}{}",
            Self::tools_said(),
            crate::domain::lessons::how()
        )
    }

    /// HQ's own tools, never as all a member has: live, a lead on Claude offered the home lab's
    /// read took this line for its whole kit and said it could not read the lab.
    fn tools_said() -> String {
        let names: Vec<String> = crate::application::protocol::tools()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .collect();
        format!(
            "Your tools at HQ: {}. Endora's own tools on the home lab (homelab_status and the \
             like) come beside them: what your tool list offers, you have.\n",
            names.join(", ")
        )
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

    /// Where the item's submission stands, for a turn tried again or an item that has one
    /// (defect remediation, C): a turn killed after its commit and before its submission came
    /// back told only to go on, and took the commit for the repair. HQ's record is the newest
    /// submission of the item's work, full head and status, or none; the node may hold one HQ
    /// never heard of, so none is said as HQ's, and submitting again is safe.
    fn submission_said(&self, run: &Run, task: TaskId) -> Done<String> {
        let finished = self.newest_finished(task)?;
        let newest = self.newest_branch(task)?;
        // A turn's briefing is written before its attempt is counted: one tried before has
        // started already, or was interrupted.
        let tried_before = run.attempts > 0 || run.interrupted > 0;
        if newest.is_none() && !tried_before {
            return Ok(String::new());
        }
        let mut line = match &finished {
            Some(b) => format!(
                "HQ's newest submission for this item: {} at {} ({}), acknowledged as finished \
                 work.",
                b.name, b.head, b.status
            ),
            None => "HQ has recorded no submission for this item as finished work; the node \
                     may hold one HQ never heard of."
                .to_owned(),
        };
        if let Some(kept) = newest.filter(|n| {
            n.status == crate::domain::wing::IN_PROGRESS
                && finished
                    .as_ref()
                    .is_none_or(|f| f.name != n.name || f.head != n.head)
        }) {
            line.push_str(&format!(
                " Kept as work in progress, not offered as finished: {} at {}.",
                kept.name, kept.head
            ));
        }
        Ok(format!(
            "{line} A commit is not a submission: a branch and full revision HQ has not \
             acknowledged as finished may still be unsubmitted. Once the work is done and \
             checked, repo_submit it; submitting a revision HQ already has records nothing new.\n"
        ))
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
        let since = self.last_turn_on(member.id, run, task.id)?;
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
        // Only the newest handover says where the work stands: older ones, and the last words
        // of turns that ran out of room before it, stay in the record and out of the briefing
        // (ADR 0142, amended 2026-10-31).
        let seen = self.thread_seen(task.id, thread)?;
        let since_seen: Vec<handover::Seen> = thread
            .iter()
            .zip(&seen)
            .filter(|(p, _)| p.post.at > since)
            .map(|(_, s)| *s)
            .collect();
        let mut new = Vec::new();
        for input in thread
            .iter()
            .zip(&seen)
            .filter(|(p, s)| p.post.at > since && **s == handover::Seen::Shown)
            .map(|(p, _)| p)
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
        if let Some(left_out) = handover::left_out_said(&since_seen) {
            said.push_str(&left_out);
        }
        Ok(said)
    }

    /// How a turn reads each post of `task`'s thread ([`handover::thread_view`]): a post is of a
    /// turn out of room when its run is one HQ judged so.
    pub(crate) fn thread_seen(
        &self,
        task: TaskId,
        thread: &[crate::application::history_ports::PostInput],
    ) -> Done<Vec<handover::Seen>> {
        let runs = self.out_of_room_runs(task)?;
        let posts: Vec<handover::ThreadPost<'_>> = thread
            .iter()
            .map(|p| handover::ThreadPost {
                author: &p.post.author,
                text: &p.post.text,
                out_of_room: matches!(p.origin,
                    crate::domain::history::InputOrigin::Run(run) if runs.contains(&run.0)),
            })
            .collect();
        Ok(handover::thread_view(&posts))
    }

    /// The thread as `my_turn` shows it: its newest `most` posts a turn reads, each read with
    /// its lineage, without older handovers and the out-of-room words before the newest (ADR
    /// 0142, amended 2026-10-31) and without the handover the briefing already tells `member`;
    /// and the line saying what was left out, if anything was.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) fn thread_for_turn(
        &self,
        run: &Run,
        member: crate::domain::MemberId,
        task: TaskId,
        most: usize,
    ) -> Done<(
        Vec<crate::application::history_ports::PostInput>,
        Option<String>,
    )> {
        let thread = self.all_authored_posts(task)?;
        let seen = self.thread_seen(task, &thread)?;
        let told = self.handover_kept_for(member, task)?;
        let mut left_out = handover::left_out_said(&seen);
        let mut shown: Vec<crate::application::history_ports::PostInput> = thread
            .into_iter()
            .zip(seen)
            .filter(|(_, s)| *s == handover::Seen::Shown)
            .map(|(p, _)| p)
            .filter(|p| told.as_deref() != Some(p.post.text.as_str()))
            .collect();
        // Past the newest `most`, a post is said to be left out, never dropped with no sign.
        let older = shown.len().saturating_sub(most);
        let shown = shown.split_off(older);
        if older > 0 {
            let (posts, them, their) = if older == 1 {
                ("post", "it", "its")
            } else {
                ("posts", "them", "their")
            };
            left_out.get_or_insert_with(String::new).push_str(&format!(
                "Left out here, kept in the record: the thread's {older} older {posts} \
                 (ask_record with task {} finds {them} by {their} words).\n",
                task.0
            ));
        }
        for input in &shown {
            self.inherit_authored_input(run.id, input.origin)?;
        }
        Ok((shown, left_out))
    }

    /// A reviewer's briefing (ADR 0142, amended 2026-10-30): who it is, the item and its
    /// criteria, who asks and how a review is done, the worker's handover (its ask for review,
    /// the evidence it cited, HQ's handover note on the item), HQ's mechanical checks in full,
    /// the owner's words on the item, and the branch: its check, its stat, its files and its
    /// diff within the reviewer's size. Not the thread's history, the map or the world: one line
    /// says which tool reads each.
    fn review_briefing(&self, member: &Member, run: &Run, task: TaskId) -> Done<String> {
        // The same fixed opening as a work turn's, then who the member is.
        let mut told = Self::opening_said();
        told.push_str(&Self::who_said(member, &self.said_today()));
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
            "Your review: {} (#{}, {}, {})\n{}\n",
            task.title,
            task.id,
            task.status.name(),
            task.priority.name(),
            task.description
        ));
        told.push_str(&self.criteria_said(&task));
        // The whole thread's lineage, as before: what the review is told is drawn from it.
        let thread = self.all_authored_posts(task.id)?;
        for input in &thread {
            self.inherit_authored_input(run.id, input.origin)?;
        }
        let members = self.store.members()?;
        let asked = thread
            .iter()
            .rev()
            .find(|p| p.post.text.contains(" \u{2192} in_review"));
        if let Some(asked) = asked {
            let p = &asked.post;
            told.push_str(&format!(
                "{} asks for your review.\n{}\nThe handover, as {} asked for it:\n- {}\n",
                p.author,
                review::REVIEW_WORK,
                p.author,
                Self::post_line(&members, &p.author, &p.text, p.unsupported)
            ));
        }
        told.push_str(&self.checking_said(member, run, &task)?);
        if let Some(note) = self.handover_on(task.id)? {
            self.unclassified_message_input(run.id)?;
            told.push_str(&format!("{note}\n"));
        }
        told.push_str(&self.mechanical_said(task.id)?);
        let owner: Vec<String> = thread
            .iter()
            .filter(|p| p.post.author == "owner")
            .map(|p| format!("- {}\n", super::one_line(&p.post.text, OWNER_LINE_MOST)))
            .collect();
        if !owner.is_empty() {
            told.push_str("The owner on this item:\n");
            for line in &owner[owner.len().saturating_sub(OWNER_POSTS_MOST)..] {
                told.push_str(line);
            }
        }
        let note = self.goal_note_briefed(task.goal)?;
        if !note.is_empty() {
            self.unclassified_message_input(run.id)?;
            told.push_str(&format!(
                "Notes from the owner for this goal: {}\n",
                super::one_line(&note, OWNER_LINE_MOST)
            ));
        }
        told.push_str(&self.branch_for_review(member, run, task.id)?);
        let others = thread
            .len()
            .saturating_sub(owner.len() + usize::from(asked.is_some()));
        told.push_str(&format!(
            "Left out of this briefing: the thread's other {others} posts (my_turn gives its \
             newest), the map (read_world world:map), the house and the lab (read_world \
             world:house, world:lab).\n"
        ));
        Ok(told)
    }

    /// The item's newest branch as a reviewer is told it: its check, its stat, the files it
    /// changes and its diff within the reviewer's size, with where the whole diff is. Other
    /// documents on the item a line each.
    fn branch_for_review(&self, member: &Member, run: &Run, task: TaskId) -> Done<String> {
        let documents = self.store.documents_of(task)?;
        if !documents.is_empty() {
            self.documents_reached(run.id, task)?;
        }
        let mut told = String::new();
        let branch = self.newest_branch(task)?;
        let document = branch.as_ref().and_then(|b| {
            let title = format!("branch {} (", b.name);
            documents
                .iter()
                .filter(|d| d.title.starts_with(&title))
                .max_by_key(|d| d.id)
        });
        let others: Vec<String> = documents
            .iter()
            .filter(|d| document.is_none_or(|kept| kept.id != d.id))
            .map(|d| format!("d{} {}", d.id, d.title))
            .collect();
        if !others.is_empty() {
            told.push_str(&format!(
                "Other documents on this item (read one with read_document): {}\n",
                others.join("; ")
            ));
        }
        let Some(branch) = branch else {
            return Ok(told);
        };
        let check = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == branch.id)
            .max_by_key(|c| c.id)
            .map(|c| {
                let verdict = match c.ok {
                    Some(true) => "passed",
                    Some(false) => "failed",
                    None => c.status.as_str(),
                };
                let summary = if c.summary.is_empty() {
                    String::new()
                } else {
                    format!(": {}", c.summary)
                };
                format!(" Check c{} {verdict}{summary}.", c.id)
            })
            .unwrap_or_default();
        told.push_str(&format!(
            "The branch: {} at {}, {} ({} files, +{} \u{2212}{}).{check}\n",
            branch.name,
            branch.head.chars().take(12).collect::<String>(),
            branch.status,
            branch.files,
            branch.plus,
            branch.minus
        ));
        let changed = self.branch_files(&branch)?.changed;
        if !changed.is_empty() {
            let more = changed.len().saturating_sub(review::FILES_MOST);
            let mut files = changed
                .iter()
                .take(review::FILES_MOST)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if more > 0 {
                files.push_str(&format!(" and {more} more"));
            }
            told.push_str(&format!("Files it changes: {files}\n"));
        }
        let Some(document) = document else {
            return Ok(told);
        };
        let (stat, diff) = document
            .text
            .split_once("\n\n")
            .unwrap_or(("", document.text.as_str()));
        if !stat.trim().is_empty() {
            told.push_str(&format!("Its stat:\n{}\n", stat.trim_end()));
        }
        if diff.trim().is_empty() {
            return Ok(told);
        }
        let (diff, cut) = review::diff_within(diff, review::diff_chars_for(member.brain));
        if cut {
            told.push_str(&format!(
                "Its diff, the start (the whole diff: read_document d{}):\n{}\n",
                document.id,
                diff.trim_end()
            ));
        } else {
            told.push_str(&format!(
                "Its diff (also read_document d{}):\n{}\n",
                document.id,
                diff.trim_end()
            ));
        }
        Ok(told)
    }

    /// When a member's last turn on a task before this one started: what is new to it is
    /// what came after.
    fn last_turn_on(
        &self,
        member: crate::domain::MemberId,
        run: &Run,
        task: crate::domain::TaskId,
    ) -> Done<Millis> {
        Ok(self
            .store
            .runs_of(task)?
            .iter()
            .filter(|r| r.member == member && r.id != run.id)
            .filter_map(|r| r.started_at)
            .max()
            .unwrap_or(Millis::MIN))
    }
}
