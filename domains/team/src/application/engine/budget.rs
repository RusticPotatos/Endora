//! The coordination budget (ADR 0133): a task that has taken more coordination turns than its
//! budget in a day with nothing changing has the talk that would start more turns held, not
//! lost, and its lead asked, once, to narrow it, split it or drop it.
//!
//! Live, 17 and 18 October 2026, one docs branch took 44 turns, 80 posts and $2.47, 28% of all
//! the spend. The ceremony guard noticed and stopped nothing. A guard that stops things is new
//! authority, so this one is tried before it is given: in **shadow** it records what it would
//! have held (a finding, in the owner's digest and on the Guards card) and holds nothing, until
//! the owner switches it on; the verdicts on its findings say whether it would have been right.
//!
//! What a task has had is never stored: it is read from the record when asked (the turns that
//! ended on it, by the ceremony guard's own labels), after the last time it changed or its lead
//! decided it. The lead's deciding is the one thing kept (`coordination_since:<task>`), and
//! whom it was asked (`coordination_asked:<task>`), so that it is asked once.

use super::health::Seen;
use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::guards::{self, Guard, GuardMode, THRESHOLDS, TurnKind, TurnSeen};
use crate::domain::money::Micros;
use crate::domain::projects::dollars;
use crate::domain::{
    EventKind, Member, MemberId, Millis, Receipt, Refusal, Task, TaskId, WakeReason,
};

/// Where HQ keeps how a guard that is tried in shadow is set.
fn mode_key(guard: Guard) -> String {
    format!("guard_mode:{}", guard.name())
}

/// Where HQ keeps when the lead of a task last decided it: the talk before is not counted.
fn since_key(task: TaskId) -> String {
    format!("coordination_since:{}", task.0)
}

/// Where HQ keeps whom a task's lead was asked about its talk, while that question stands.
fn asked_key(task: TaskId) -> String {
    format!("coordination_asked:{}", task.0)
}

/// The key a task's finding is kept under: one finding for the guard and the task.
fn finding_key(task: TaskId) -> String {
    format!("coordination:{}", task.0)
}

/// How much talk a task has had with nothing coming of it, and what it cost.
struct Talk {
    /// Its coordination turns within the window, after its last change and its lead's last
    /// decision.
    turns: usize,
    /// What those turns spent.
    spent: Micros,
    /// Its coordination turns over the week: what a finding rests on.
    week: usize,
}

impl Talk {
    /// What a task's turns come to as of `now`, `since` its lead last decided it.
    fn of(turns: &[TurnSeen], now: Millis, since: Millis) -> Self {
        let counted = guards::talk_turns(turns, now, since);
        Self {
            turns: counted.len(),
            spent: counted.iter().map(|t| t.spent).sum(),
            week: turns
                .iter()
                .filter(|t| guards::turn_kind(t) == TurnKind::Coordination)
                .count(),
        }
    }

    /// Whether it is more than a task's budget.
    fn over(&self) -> bool {
        guards::over_budget(self.turns)
    }

    /// The mark a finding rests on: the week's talk, which contains the day's.
    fn mark(&self) -> i64 {
        i64::try_from(self.week).unwrap_or(i64::MAX)
    }
}

/// What the finding on a task over its budget says, by what was done about it.
enum How {
    /// The guard is in shadow: it would have held.
    Would,
    /// The guard is on, and the lead of the area was asked.
    Held(String),
    /// The guard is on, and nobody leads the work: there is nobody to decide it, so nothing is
    /// held.
    Nobody,
}

impl How {
    fn said(&self, task: TaskId, talk: &Talk) -> String {
        let head = format!(
            "on #{} (budget {}, {} turns, {})",
            task.0,
            THRESHOLDS.coordination_budget,
            talk.turns,
            dollars(talk.spent)
        );
        match self {
            Self::Would => format!("would have held coordination {head}"),
            Self::Held(lead) => {
                format!("held coordination {head}: {lead} was asked to narrow, split or drop it")
            }
            Self::Nobody => {
                format!("would have held coordination {head}: nobody leads it, so nothing is held")
            }
        }
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the mode -----------------------------------------------------------------------------

    /// How a guard is set to behave now (ADR 0133): a guard that is tried in shadow starts there,
    /// until the owner switches it on; the rest are always on.
    ///
    /// # Errors
    /// A store failure.
    pub fn guard_mode(&self, guard: Guard) -> Done<GuardMode> {
        Ok(self
            .store
            .setting(&mode_key(guard))?
            .and_then(|m| GuardMode::from_name(&m))
            .filter(|_| guard.has_shadow())
            .unwrap_or_else(|| guard.starts_in()))
    }

    /// The owner switches a guard that is tried in shadow on, or back to shadow (ADR 0133). Back
    /// in shadow, what it held goes at the next tick.
    ///
    /// # Errors
    /// [`Refusal::NoShadowMode`] for a guard that only records; a store failure.
    pub fn set_guard_mode(&self, guard: Guard, mode: GuardMode) -> Done<()> {
        let _held = self.held();
        if !guard.has_shadow() {
            return Err(EngineError::Refused(Refusal::NoShadowMode(guard.name())));
        }
        self.store.set_setting(&mode_key(guard), mode.name())?;
        self.emit(
            EventKind::GuardMode,
            serde_json::json!({ "guard": guard.name(), "mode": mode.name() }),
        )
    }

    // ---- what a task has had ------------------------------------------------------------------

    /// When the lead of `task` last decided it; nothing before counts against its budget.
    fn coordination_since(&self, task: TaskId) -> Done<Millis> {
        Ok(self
            .store
            .setting(&since_key(task))?
            .and_then(|s| s.parse().ok())
            .unwrap_or(Millis::MIN))
    }

    /// The talk `task` has had as of now, from the record.
    fn talk_of(&self, task: TaskId, now: Millis) -> Done<Talk> {
        let seen: Vec<TurnSeen> = self
            .turns_seen(now, Some(task))?
            .into_iter()
            .map(|s| s.turn)
            .collect();
        Ok(Talk::of(&seen, now, self.coordination_since(task)?))
    }

    /// Whether talk about `task` is held now: the guard is on, the task is open, it has a lead
    /// to decide it, and it has had more talk than its budget with nothing changing. Asked of a
    /// wake that only asks for talk (ADR 0133).
    pub(super) fn talk_is_held(&self, task: TaskId) -> Done<bool> {
        if self.guard_mode(Guard::Coordination)? != GuardMode::On {
            return Ok(false);
        }
        let Some(found) = self.store.task(task)? else {
            return Ok(false);
        };
        if found.status.finished() || self.area_lead(&found)?.is_none() {
            return Ok(false);
        }
        Ok(self.talk_of(task, self.clock.now())?.over())
    }

    /// The lead of a task decided it: narrowed it, split it or decided it. The talk before is not
    /// counted against its budget again, and the question to the lead is answered (ADR 0133).
    pub(super) fn renew_coordination_budget(&self, task: TaskId) -> Done<()> {
        self.store
            .set_setting(&since_key(task), &self.clock.now().to_string())?;
        self.store.set_setting(&asked_key(task), "")?;
        Ok(())
    }

    /// Whether `member` was asked what to do about `task`'s talk, and has not yet decided it.
    pub(super) fn asked_about_talk(&self, task: TaskId, member: MemberId) -> Done<bool> {
        Ok(self
            .store
            .setting(&asked_key(task))?
            .is_some_and(|s| s == member.0.to_string()))
    }

    // ---- over the budget ----------------------------------------------------------------------

    /// A talk wake was just held because its task is over its budget: its finding is kept, and
    /// its lead is asked, once (ADR 0133).
    pub(super) fn coordination_held_notice(&self, task: TaskId) -> Done<()> {
        let current = self.task(task)?;
        let talk = self.talk_of(task, self.clock.now())?;
        if !talk.over() {
            return Ok(());
        }
        self.coordination_over(&current, &talk, GuardMode::On)
    }

    /// A task over its budget, as the guard is set: in shadow the finding says what would have
    /// been held and nothing else happens; on, with a lead to decide it, the finding says what
    /// was held and the lead is asked, once. Where nobody leads the work nothing can be held, and
    /// the finding says so.
    fn coordination_over(&self, task: &Task, talk: &Talk, mode: GuardMode) -> Done<()> {
        let lead = self.area_lead(task)?;
        let project = task.project.map(|p| p.0);
        let key = finding_key(task.id);
        let (Some(lead), GuardMode::On) = (lead, mode) else {
            let how = match mode {
                GuardMode::Shadow => How::Would,
                GuardMode::On => How::Nobody,
            };
            self.find_noticed(
                Guard::Coordination,
                &key,
                Some(task.id),
                project,
                &how.said(task.id, talk),
                talk.mark(),
            )?;
            // Nobody is asked in shadow, or where nobody leads; if the guard was on a moment
            // ago, the question it asked has no standing now.
            self.store.set_setting(&asked_key(task.id), "")?;
            return Ok(());
        };
        let evidence = How::Held(lead.name.clone()).said(task.id, talk);
        let found = self.find_noticed(
            Guard::Coordination,
            &key,
            Some(task.id),
            project,
            &evidence,
            talk.mark(),
        )?;
        self.ask_the_lead_about_talk(task, talk, &lead, found.id)
    }

    /// Puts the one decision to the lead of a task whose talk is over its budget: said in the
    /// thread, with the three ways out, and the lead woken. Once while the question stands.
    fn ask_the_lead_about_talk(
        &self,
        task: &Task,
        talk: &Talk,
        lead: &Member,
        finding: i64,
    ) -> Done<()> {
        let marker = asked_key(task.id);
        if self.store.setting(&marker)?.is_some_and(|s| !s.is_empty()) {
            return Ok(());
        }
        self.store.set_setting(&marker, &lead.id.0.to_string())?;
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "Talk on this task is over its budget (finding {finding}): {} turns of \
                 coordination in 24 h with nothing changed, {} spent. Wakes that only ask for \
                 more talk about it are held, not lost, until it is decided. {}, decide it: \
                 narrow its criteria (narrow_criteria), split it (create_task) or drop it \
                 (decide_stuck), on evidence. Then say whether the finding was real with \
                 judge_finding.",
                talk.turns,
                dollars(talk.spent),
                lead.name
            ),
        )?;
        self.wake_held_lock(lead.id, Some(task.id), WakeReason::Guard)?;
        Ok(())
    }

    // ---- the look -----------------------------------------------------------------------------

    /// Every open task's talk over the day, as of the process-health look (ADR 0133): over its
    /// budget, a finding (and, with the guard on, its lead asked, once); back under it, finished
    /// or decided, the finding clears with its card, and what was held goes at the next tick.
    pub(super) fn look_at_coordination(&self, now: Millis, week: &[Seen]) -> Done<()> {
        let mode = self.guard_mode(Guard::Coordination)?;
        let mut over: Vec<TaskId> = Vec::new();
        for task in self.store.tasks()?.iter().filter(|t| !t.status.finished()) {
            let mine: Vec<TurnSeen> = week
                .iter()
                .filter(|s| s.task == task.id)
                .map(|s| s.turn)
                .collect();
            // The day's talk is inside the week's: no more than the budget over the week cannot
            // be more than the budget over the day.
            let weeks_talk = mine
                .iter()
                .filter(|t| guards::turn_kind(t) == TurnKind::Coordination)
                .count();
            if weeks_talk <= THRESHOLDS.coordination_budget {
                continue;
            }
            let talk = Talk::of(&mine, now, self.coordination_since(task.id)?);
            if !talk.over() {
                continue;
            }
            over.push(task.id);
            self.coordination_over(task, &talk, mode)?;
        }
        for finding in self.store.findings()? {
            if finding.guard != Guard::Coordination || finding.cleared_at.is_some() {
                continue;
            }
            let Some(task) = finding.task.map(TaskId) else {
                self.clear(&finding)?;
                continue;
            };
            if over.contains(&task) {
                continue;
            }
            self.clear(&finding)?;
            self.store.set_setting(&asked_key(task), "")?;
        }
        Ok(())
    }

    /// The tick's step before turns start: a wake held for a task that has no turn live on it
    /// has nothing to wait behind, and is asked again (ADR 0133). Held talk goes once the budget
    /// is back in hand: the day has passed, the lead decided, the guard went back to shadow.
    /// Still over, it stays held, as a wake behind a live turn does.
    pub(super) fn release_held_talk(&self) -> Done<()> {
        let mut held: Vec<TaskId> = Vec::new();
        for wake in self.store.wakes()? {
            if let (Receipt::Held, Some(task)) = (wake.receipt, wake.task) {
                if !held.contains(&task) {
                    held.push(task);
                }
            }
        }
        if held.is_empty() {
            return Ok(());
        }
        let live: Vec<Option<TaskId>> = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.status.live())
            .map(|r| r.task)
            .collect();
        for task in held {
            if live.contains(&Some(task)) {
                continue;
            }
            self.release_held(task)?;
        }
        Ok(())
    }
}
