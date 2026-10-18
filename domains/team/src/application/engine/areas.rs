//! Strategy per goal, each area's measures, and the leads reporting up (ADR 0120).
//!
//! - **Strategy** is the CEO's: per goal, its purpose, what success looks like, its measures,
//!   its current bets and what is not done now. The owner edits it on the goal card.
//! - **Measures** are each area's, computed from HQ's record ([`scorecard::lab_measures`],
//!   [`scorecard::branch_measures`]): the CEO's briefings use them, not raw readings.
//! - **Leads report up**: a daily "State of my area" and a weekly "Patterns" per lead, each a
//!   routine made once. The report is kept as a report (`report_area`), never as work on the
//!   board or the scorecard; the patterns turn proposes, and the CEO decides.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::{Author, Done, Engine, EngineError, alarm_in};
use crate::application::ports::{Clock, NewRoutine, Run, TeamStore, WorkerAdapter};
use crate::application::scorecard::{self, Area, AreaRecord, Window};
use crate::domain::areas::{self, AreaReport, Strategy, StrategyEdit};
use crate::domain::routines::DAY_MS;
use crate::domain::{
    EventKind, GoalId, GoalStatus, Member, MemberId, Millis, Ref, Refusal, Role, RoutineId, RunId,
    Task, TaskId, TaskStatus,
};

/// What a lead's daily report asks of it.
pub const REPORT_ASKS: &str = "Report on your area to the CEO, once a day. Read your \
    briefing (your projects and their money, your area's open work, its alarms and its \
    measures), then call report_area once, four short parts: moved (what moved, with task \
    numbers), stuck (what is stuck, and on what), need (what you need, and from whom), and \
    proposals (what you propose; make each with propose_project, on evidence). It is a report, \
    not work: report_area closes it.";

/// What a lead's weekly patterns turn asks of it.
pub const PATTERNS_ASKS: &str = "Look for what keeps coming back in your area, once a week. \
    Read your briefing's repeats (the last 30 days' tasks by the containers they named, and \
    the alarms that came back). Where work repeats, propose a project with propose_project, \
    citing its task numbers and event ids: the CEO decides. Where nothing repeats, propose \
    nothing. Then close this as done, saying what you proposed.";

/// What the CEO's daily review asks of it.
pub const DAILY_REVIEW: &str = "Review the house once a day, from your briefing: the leads' \
    reports, each area's measures, the proposals waiting, and each goal's strategy. Where a \
    goal has no strategy, write it with set_strategy (purpose, success, measures, bets, \
    not_now), on evidence. Decide each proposal with decide_project (yes, no, or not now). \
    Where an area needs a steer, direct its lead (direct). Note what you decided with \
    note_house. Put to the board only what the owner alone decides (ask_owner). Then close \
    this review as done, saying what you decided.";

/// Where HQ keeps each lead's report routine, once it made it.
const REPORT_KEY: &str = "area_report_routine:";
/// Where HQ keeps each lead's patterns routine, once it made it.
const PATTERNS_KEY: &str = "area_patterns_routine:";

/// How far back an area's patterns look.
const PATTERNS_LOOK_BACK_MS: Millis = 30 * DAY_MS;

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

    fn write_strategy(&self, goal: GoalId, edit: &StrategyEdit, by: &str) -> Done<Strategy> {
        self.store.goal(goal)?.ok_or(EngineError::Unknown("goal"))?;
        let written = self
            .strategy_of(goal)?
            .edited(edit, by, self.clock.now())
            .map_err(EngineError::Refused)?;
        self.store.save_strategy(&written)?;
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
        let written = self.write_strategy(goal, edit, &ceo.name)?;
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
        self.write_strategy(goal, edit, "owner")
    }

    // ---- the leads' routines and reports ------------------------------------------------

    /// Makes each lead's "State of my area" (daily, due at the owner's 18:00) and "Patterns"
    /// (weekly), once per lead (a setting remembers each), under the goal of the first
    /// active project it leads; a lead that leads none waits until it does. Returns what it
    /// made.
    ///
    /// # Errors
    /// What [`Self::add_routine`] refuses; a store failure.
    pub fn seed_area_routines(&self) -> Done<Vec<RoutineId>> {
        let mut made = Vec::new();
        let projects = self.store.projects()?;
        for lead in self.store.members()? {
            if lead.role != Role::Lead || lead.furloughed() {
                continue;
            }
            let Some(project) = projects
                .iter()
                .find(|p| p.lead == lead.id && p.status.takes_work())
            else {
                continue;
            };
            for (key, title, asks, every) in [
                (REPORT_KEY, areas::REPORT_ROUTINE, REPORT_ASKS, DAY_MS),
                (
                    PATTERNS_KEY,
                    areas::PATTERNS_ROUTINE,
                    PATTERNS_ASKS,
                    7 * DAY_MS,
                ),
            ] {
                let setting = format!("{key}{}", lead.id.0);
                if self.store.setting(&setting)?.is_some() {
                    continue;
                }
                let id = self.add_routine(
                    Author::Endora,
                    NewRoutine {
                        title: title.to_owned(),
                        description: asks.to_owned(),
                        goal: project.goal,
                        project: None,
                        assignee: lead.id,
                        every_ms: every,
                        on_lab_trouble: false,
                    },
                )?;
                if key == REPORT_KEY {
                    let mut routine = self.routine(id)?;
                    routine.next_due_at = areas::next_at_hour(
                        self.clock.now(),
                        self.settings.offset_minutes,
                        areas::REPORT_HOUR,
                    );
                    self.store.save_routine(&routine)?;
                }
                self.store.set_setting(&setting, &id.0.to_string())?;
                made.push(id);
            }
        }
        Ok(made)
    }

    /// Whether `routine` (for `assignee`) is a lead's report or patterns routine, and which.
    pub(super) fn report_kind(
        &self,
        routine: RoutineId,
        assignee: MemberId,
    ) -> Done<Option<&'static str>> {
        for (key, kind) in [(REPORT_KEY, "report"), (PATTERNS_KEY, "patterns")] {
            let kept = self.store.setting(&format!("{key}{}", assignee.0))?;
            if kept.as_deref() == Some(routine.0.to_string().as_str()) {
                return Ok(Some(kind));
            }
        }
        Ok(None)
    }

    /// The tasks that are a lead's report or patterns turn, not work on the board.
    pub(super) fn report_task_ids(&self) -> Done<HashSet<TaskId>> {
        Ok(self
            .store
            .report_tasks()?
            .into_iter()
            .map(|(t, _, _)| t)
            .collect())
    }

    /// The kind of report `task` is, if it is one.
    fn report_task_kind(&self, task: TaskId) -> Done<Option<String>> {
        Ok(self
            .store
            .report_tasks()?
            .into_iter()
            .find(|(t, _, _)| *t == task)
            .map(|(_, _, kind)| kind))
    }

    /// A lead's turn on its "State of my area" files its report (moved, stuck, need,
    /// proposals) and so closes it. Kept as the lead's area report, which the CEO's daily
    /// review reads; said in the feed.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NotAReport`] for anyone but a lead in its own
    /// report's turn; [`Refusal::EmptyReport`]; [`Refusal::TooLong`].
    pub fn report_area_in(&self, run: RunId, parts: [&str; 4]) -> Done<i64> {
        let _held = self.held();
        let turn = self.running(run)?;
        let lead = self.member(turn.member)?;
        let Some(task) = turn.task else {
            return Err(EngineError::Refused(Refusal::NotAReport));
        };
        let current = self.task(task)?;
        let theirs = current.owner.or(current.assignee) == Some(lead.id);
        let is_report = self.report_task_kind(task)?.as_deref() == Some("report");
        if lead.role != Role::Lead || !theirs || !is_report || current.status.finished() {
            return Err(EngineError::Refused(Refusal::NotAReport));
        }
        let report = AreaReport::new(lead.id, parts, Some(turn.id), Some(task), self.clock.now())
            .map_err(EngineError::Refused)?;
        let id = self.store.add_area_report(&report)?;
        self.post_held_lock(
            Author::Member(lead.id),
            task,
            &format!("Area report:\n{}", report.said().trim_end()),
        )?;
        self.store.set_task_status(task, TaskStatus::Done)?;
        self.emit_status(&current, TaskStatus::Done)?;
        self.mark_progress(lead.id, task)?;
        self.emit(
            EventKind::AreaReported,
            json!({ "report": id, "lead": lead.id.0, "task": task.0 }),
        )?;
        Ok(id)
    }

    /// Each lead's latest report, for the screen.
    pub(super) fn reports_shown(&self) -> Done<serde_json::Value> {
        let mut latest: HashMap<MemberId, AreaReport> = HashMap::new();
        for report in self.store.area_reports()? {
            latest.insert(report.lead, report);
        }
        let mut shown: Vec<&AreaReport> = latest.values().collect();
        shown.sort_by_key(|r| r.lead.0);
        Ok(json!(
            shown
                .iter()
                .map(|r| json!({ "lead": r.lead.0, "at": r.at, "moved": r.moved,
                                 "stuck": r.stuck, "need": r.need, "proposals": r.proposals }))
                .collect::<Vec<_>>()
        ))
    }

    // ---- areas and their measures -------------------------------------------------------

    /// Each lead's area, from the record: the lead and whoever reports to it, the tasks of
    /// the projects it leads, and which measures apply.
    pub(super) fn areas(&self) -> Done<Vec<(Member, Area)>> {
        let members = self.store.members()?;
        let projects = self.store.projects()?;
        let tasks = self.store.tasks()?;
        let routines = self.store.routines()?;
        let branches = self.store.branches()?;
        let wing = self.wing_leads()?;
        let mut all = Vec::new();
        for lead in members
            .iter()
            .filter(|m| m.role == Role::Lead && !m.furloughed())
        {
            let team: Vec<MemberId> = members
                .iter()
                .filter(|m| crate::domain::staffing::lead_of(m, &members) == Some(lead.id))
                .map(|m| m.id)
                .collect();
            let led: Vec<_> = projects
                .iter()
                .filter(|p| p.lead == lead.id)
                .map(|p| p.id)
                .collect();
            let area_tasks: HashSet<TaskId> = tasks
                .iter()
                .filter(|t| t.project.is_some_and(|p| led.contains(&p)))
                .map(|t| t.id)
                .collect();
            let lab = routines
                .iter()
                .any(|r| r.on_lab_trouble && r.project.is_some_and(|p| led.contains(&p)));
            let works_in_branches =
                wing.contains(&lead.id) || branches.iter().any(|b| team.contains(&b.member));
            all.push((
                lead.clone(),
                Area {
                    lead: lead.id,
                    members: team,
                    tasks: area_tasks,
                    lab,
                    branches: works_in_branches,
                },
            ));
        }
        Ok(all)
    }

    /// An area's measures in a window, as the CEO and the lead read them; empty when none
    /// apply.
    pub(super) fn measures_said(&self, area: &Area, window: &Window) -> Done<Vec<String>> {
        let events = self.store.events_after(0)?;
        let tasks = self.store.tasks()?;
        let desk = self.store.desk()?;
        let branches = self.store.branches()?;
        let members = self.store.members()?;
        let testers: Vec<String> = members
            .iter()
            .filter(|m| m.role == Role::Reviewer)
            .map(|m| m.name.clone())
            .collect();
        let mut threads = HashMap::new();
        for branch in branches.iter().filter(|b| area.members.contains(&b.member)) {
            let posts: Vec<(String, String)> = self
                .store
                .thread(branch.task)?
                .into_iter()
                .map(|p| (p.author, p.text))
                .collect();
            threads.insert(branch.task, posts);
        }
        let lab = self.lab_names();
        let record = AreaRecord {
            events: &events,
            tasks: &tasks,
            desk: &desk,
            branches: &branches,
            threads: &threads,
            testers: &testers,
            lab: lab.as_deref(),
            now: self.clock.now(),
        };
        let mut said = Vec::new();
        if area.lab {
            said.push(format!(
                "the lab: {}",
                scorecard::lab_measures(area, &record, window).said()
            ));
        }
        if area.branches {
            said.push(format!(
                "branches: {}",
                scorecard::branch_measures(area, &record, window).said()
            ));
        }
        Ok(said)
    }

    /// The last `days` days, by the owner's day.
    fn days(&self, days: i64) -> Window {
        Window::days_ending(self.today(), days, self.settings.offset_minutes)
    }

    /// Each area's measures for the scorecard: today and the last seven days.
    pub(super) fn areas_shown(&self) -> Done<serde_json::Value> {
        let events = self.store.events_after(0)?;
        let tasks = self.store.tasks()?;
        let desk = self.store.desk()?;
        let branches = self.store.branches()?;
        let members = self.store.members()?;
        let testers: Vec<String> = members
            .iter()
            .filter(|m| m.role == Role::Reviewer)
            .map(|m| m.name.clone())
            .collect();
        let lab = self.lab_names();
        let mut shown = Vec::new();
        for (lead, area) in self.areas()? {
            let mut threads = HashMap::new();
            for branch in branches.iter().filter(|b| area.members.contains(&b.member)) {
                let posts: Vec<(String, String)> = self
                    .store
                    .thread(branch.task)?
                    .into_iter()
                    .map(|p| (p.author, p.text))
                    .collect();
                threads.insert(branch.task, posts);
            }
            let record = AreaRecord {
                events: &events,
                tasks: &tasks,
                desk: &desk,
                branches: &branches,
                threads: &threads,
                testers: &testers,
                lab: lab.as_deref(),
                now: self.clock.now(),
            };
            let window = |days: i64| self.days(days);
            let lab_of = |w: &Window| {
                area.lab
                    .then(|| scorecard::lab_measures(&area, &record, w).json())
            };
            let branches_of = |w: &Window| {
                area.branches
                    .then(|| scorecard::branch_measures(&area, &record, w).json())
            };
            shown.push(json!({
                "lead": lead.id.0, "name": lead.name,
                "members": area.members.iter().map(|m| m.0).collect::<Vec<_>>(),
                "today": { "lab": lab_of(&window(1)), "branches": branches_of(&window(1)) },
                "week": { "lab": lab_of(&window(7)), "branches": branches_of(&window(7)) },
            }));
        }
        Ok(json!(shown))
    }

    // ---- what the leads are told on their report and patterns turns -----------------------

    /// What a lead's report or patterns turn is shown about its area; empty for any other
    /// turn.
    pub(super) fn area_picture(&self, lead: &Member, run: &Run) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        let Some(kind) = self.report_task_kind(task)? else {
            return Ok(String::new());
        };
        let Some((_, area)) = self.areas()?.into_iter().find(|(m, _)| m.id == lead.id) else {
            return Ok(String::new());
        };
        if kind == "patterns" {
            return self.area_patterns(&area);
        }
        let tasks = self.store.tasks()?;
        let open: Vec<&Task> = tasks
            .iter()
            .filter(|t| area.tasks.contains(&t.id) && !t.status.finished())
            .collect();
        let mut told = format!("Your area's open work ({}):\n", open.len());
        if open.is_empty() {
            told.push_str("- none\n");
        }
        for t in open.iter().take(15) {
            told.push_str(&format!("- #{} {} ({})\n", t.id, t.title, t.status.name()));
        }
        let since = self.clock.now() - DAY_MS;
        let alarms = self
            .store
            .events_after(0)?
            .into_iter()
            .filter(|e| e.at >= since && alarm_in(e).is_some())
            .filter(|e| {
                e.data["task"]
                    .as_i64()
                    .is_some_and(|t| area.tasks.contains(&TaskId(t)))
            })
            .count();
        told.push_str(&format!(
            "Alarms in your area in the last 24 hours: {alarms}.\n"
        ));
        let measures = self.measures_said(&area, &self.days(7))?;
        if !measures.is_empty() {
            told.push_str(&format!(
                "Your area's measures, the last 7 days: {}.\n",
                measures.join("; ")
            ));
        }
        Ok(told)
    }

    /// The area's repeats: its last 30 days' tasks by the containers they named, and the
    /// alarms about its work that came back.
    fn area_patterns(&self, area: &Area) -> Done<String> {
        let since = self.clock.now() - PATTERNS_LOOK_BACK_MS;
        let events = self.store.events_after(0)?;
        let in_area = |e: &crate::domain::Event| {
            e.data["task"]
                .as_i64()
                .is_some_and(|t| area.tasks.contains(&TaskId(t)))
        };
        let made: Vec<TaskId> = events
            .iter()
            .filter(|e| e.at >= since && e.kind == "task.created" && in_area(e))
            .filter_map(|e| e.data["task"].as_i64().map(TaskId))
            .collect();
        let alarms: Vec<(i64, &str)> = events
            .iter()
            .filter(|e| e.at >= since && in_area(e))
            .filter_map(|e| Some((e.id, alarm_in(e)?)))
            .collect();
        let Some(lab) = self.lab_names() else {
            return Ok(
                "Your area's repeats: the node has not said yet what containers the home lab \
                 has.\n"
                    .to_owned(),
            );
        };
        Ok(format!(
            "Your area's repeats:\n{}",
            self.patterns(&made, &alarms, &lab)?
        ))
    }

    // ---- what the CEO is told ---------------------------------------------------------------

    /// The goals' strategies, for the CEO: each open goal's, said in full, marked when it
    /// changed since `since`, and asked for when it is empty; with its notes and lessons.
    pub(super) fn strategies_said(&self, since: Option<Millis>) -> Done<String> {
        let mut told = String::from("Goals and their strategy:\n");
        let goals: Vec<_> = self
            .store
            .goals()?
            .into_iter()
            .filter(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Proposed))
            .collect();
        if goals.is_empty() {
            told.push_str("- none\n");
        }
        for goal in goals {
            told.push_str(&format!(
                "- #{} {} ({})\n",
                goal.id,
                goal.title,
                goal.status.name()
            ));
            let strategy = self.strategy_of(goal.id)?;
            if strategy.is_empty() {
                told.push_str(
                    "  No strategy yet: write it with set_strategy (purpose, success, \
                     measures, bets, not_now), on evidence.\n",
                );
            } else {
                if since.is_some_and(|at| strategy.updated_at > at) {
                    told.push_str(&format!(
                        "  (changed since your last turn, by {})\n",
                        strategy.by
                    ));
                }
                told.push_str(&strategy.said());
            }
            let notes = self.goal_note_briefed(goal.id)?;
            if !notes.trim().is_empty() {
                told.push_str("  Its notes and lessons:\n");
                for line in notes.lines().filter(|l| !l.trim().is_empty()) {
                    told.push_str(&format!("  {}\n", line.trim()));
                }
            }
        }
        Ok(told)
    }

    /// The leads' latest reports, for the CEO.
    pub(super) fn reports_said(&self) -> Done<String> {
        let members = self.store.members()?;
        let reports = self.store.area_reports()?;
        let mut told = String::from("The leads' latest reports:\n");
        let leads: Vec<&Member> = members
            .iter()
            .filter(|m| m.role == Role::Lead && !m.furloughed())
            .collect();
        if leads.is_empty() {
            told.push_str("- no lead\n");
        }
        for lead in leads {
            let Some(latest) = reports.iter().rev().find(|r| r.lead == lead.id) else {
                told.push_str(&format!("- {}: no report yet\n", lead.name));
                continue;
            };
            let (d, m) = crate::domain::lessons::day_and_month(crate::domain::money::day_of(
                latest.at,
                self.settings.offset_minutes,
            ));
            told.push_str(&format!("- {}, {d} {m}:\n{}", lead.name, latest.said()));
        }
        Ok(told)
    }

    /// Each area's measures over the last `days` days, for the CEO.
    pub(super) fn areas_said(&self, days: i64) -> Done<String> {
        let mut told = format!("How each area is doing, the last {days} days:\n");
        let all = self.areas()?;
        if all.is_empty() {
            told.push_str("- no area\n");
        }
        for (lead, area) in all {
            let said = self.measures_said(&area, &self.days(days))?;
            let line = if said.is_empty() {
                "no measures yet".to_owned()
            } else {
                said.join("; ")
            };
            told.push_str(&format!("- {}'s area: {line}\n", lead.name));
        }
        Ok(told)
    }

    /// The proposals waiting for the CEO, each with its evidence.
    pub(super) fn proposals_said(&self) -> Done<String> {
        let members = self.store.members()?;
        let name_of = |id: MemberId| {
            members
                .iter()
                .find(|m| m.id == id)
                .map_or("nobody", |m| m.name.as_str())
        };
        let mut told = String::from("Project proposals waiting for your decision:\n");
        let waiting: Vec<_> = self
            .store
            .projects()?
            .into_iter()
            .filter(|p| p.status == crate::domain::ProjectStatus::Proposed)
            .collect();
        if waiting.is_empty() {
            told.push_str("- none\n");
        }
        for p in waiting {
            let asks = p
                .asked_cap
                .map(|c| format!(", asks {} a month", crate::domain::projects::dollars(c)))
                .unwrap_or_default();
            let evidence: Vec<String> = p.evidence.iter().map(|e| e.said()).collect();
            told.push_str(&format!(
                "- #{} {} (by {}, for goal #{}, led by {}{asks}): {} Evidence: {}.\n",
                p.id,
                p.title,
                p.created_by,
                p.goal,
                name_of(p.lead),
                p.reason,
                evidence.join(", ")
            ));
        }
        Ok(told)
    }

    /// The budget split: each goal's projects with this month's spend against their caps,
    /// and the caps together against the pool's month.
    pub(super) fn budget_said(&self) -> Done<String> {
        let members = self.store.members()?;
        let mut told = String::from("The budget this month, by goal and project:\n");
        let mut caps = 0;
        for project in self.store.projects()? {
            if !project.status.holds_its_cap() {
                continue;
            }
            caps += project.cap.unwrap_or(0);
            told.push_str(&format!(
                "- goal #{}: {}\n",
                project.goal,
                self.project_line(&project, &members)?
            ));
        }
        told.push_str(&format!(
            "Caps together: {} of the pool's {} a month.\n",
            crate::domain::projects::dollars(caps),
            crate::domain::projects::dollars(self.settings.pool_per_month)
        ));
        Ok(told)
    }
}
