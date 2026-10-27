//! Strategy per goal, each area's measures, and the leads reporting up (ADR 0120).
//!
//! - **Strategy** is the CEO's: per goal, its purpose, what success looks like, its measures,
//!   its current bets and what is not done now. The owner edits it on the goal card.
//! - **Measures** are each area's, computed from HQ's record ([`scorecard::lab_measures`],
//!   [`scorecard::branch_measures`]): the CEO's briefings use them, not raw readings.
//! - **Leads report up**: a daily "State of my area" and a weekly "Patterns" per lead, each a
//!   routine made once. The report is kept as a report (`report_area`), never as work on the
//!   board or the scorecard; the patterns turn proposes, and the CEO decides.

use std::collections::HashSet;

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::areas::{Strategy, StrategyEdit};
use crate::domain::{EventKind, GoalId, Ref, RunId, TaskId};

/// Where HQ keeps who wrote a goal's "not now": `owner` or `run:<id>`.
const NOT_NOW_ORIGIN: &str = "not_now_origin:";

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- strategy -------------------------------------------------------------------------

    /// A goal's strategy: what is written, or an empty one.
    ///
    /// # Errors
    /// A store failure.
    pub fn strategy_of(&self, goal: GoalId) -> Done<Strategy> {
        Ok(self
            .store
            .strategies()?
            .into_iter()
            .find(|s| s.goal == goal)
            .unwrap_or_else(|| Strategy::empty(goal)))
    }

    fn write_strategy(
        &self,
        goal: GoalId,
        edit: &StrategyEdit,
        by: &str,
        origin: &str,
    ) -> Done<Strategy> {
        self.store.goal(goal)?.ok_or(EngineError::Unknown("goal"))?;
        let written = self
            .strategy_of(goal)?
            .edited(edit, by, self.clock.now())
            .map_err(EngineError::Refused)?;
        self.store.save_strategy(&written)?;
        // Who wrote "not now", which every briefing on the goal shows (ADR 0138 A2c): `owner`,
        // or `run:<id>` for the CEO's turn.
        if edit.not_now.is_some() {
            self.store
                .set_setting(&format!("{NOT_NOW_ORIGIN}{}", goal.0), origin)?;
        }
        self.emit(EventKind::StrategySet, json!({ "goal": goal.0, "by": by }))?;
        Ok(written)
    }

    /// The CEO's turn writes a goal's strategy, a part at a time, on evidence.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`]; what the evidence rule refuses;
    /// [`Refusal::NotAStrategy`]; [`Refusal::TooLong`]; [`EngineError::Unknown`] for a goal
    /// there is not.
    pub fn set_strategy_in(
        &self,
        run: RunId,
        goal: GoalId,
        edit: &StrategyEdit,
        cited: &[Ref],
    ) -> Done<Strategy> {
        let _held = self.held();
        let (turn, ceo) = self.the_ceos_turn(run)?;
        self.judge_decision(cited, turn.id)?;
        let written = self.write_strategy(goal, edit, &ceo.name, &format!("run:{}", turn.id.0))?;
        if let Some(task) = turn.task {
            self.mark_progress(ceo.id, task)?;
        }
        Ok(written)
    }

    /// The owner writes a goal's strategy on the goal card.
    ///
    /// # Errors
    /// As [`Self::set_strategy_in`], but for the turn and the evidence.
    pub fn set_strategy(&self, goal: GoalId, edit: &StrategyEdit) -> Done<Strategy> {
        let _held = self.held();
        self.write_strategy(goal, edit, "owner", "owner")
    }

    // ---- the leads' routines and reports ------------------------------------------------

    /// The tasks that are a lead's report or patterns turn, not work on the board.
    pub(super) fn report_task_ids(&self) -> Done<HashSet<TaskId>> {
        Ok(self
            .store
            .report_tasks()?
            .into_iter()
            .map(|(t, _, _)| t)
            .collect())
    }

    // ---- areas and their measures -------------------------------------------------------

    // ---- what the leads are told on their report and patterns turns -----------------------

    // ---- what the CEO is told ---------------------------------------------------------------
}
