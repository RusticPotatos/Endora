//! Projects, their budgets, proposals, and whom each member reports to (ADR 0113).
//!
//! A project serves one goal and has a lead; a task may be in one, and its sub-tasks stay in
//! it. The owner may give a project a monthly cap: its Claude turns are held to what is left of
//! it, the owner hears once at 80%, and at 100% its Claude turns wait (a budget pause, which
//! lifts itself when the month turns, or when the owner raises the cap). A lead proposes a
//! project with evidence from HQ's own record; the CEO decides; a cap asked for goes to the
//! owner's desk with the CEO's recommendation. Every rule is here, in code: nothing a model
//! says is taken for a check.

use serde_json::json;

use super::{Author, Done, Engine, EngineError, one_line};
use crate::application::ports::{Clock, DeskItem, NewProject, NewTask, TeamStore, WorkerAdapter};
use crate::domain::projects::{self, Level, dollars};
use crate::domain::{
    EventKind, Evidence, GoalId, GoalStatus, Member, MemberId, Micros, Millis, Pause, Priority,
    Project, ProjectId, ProjectStatus, Refusal, Role, RunId, RunStatus, Task, TaskId, WakeReason,
};

/// The project HQ makes, once, for the home lab's routines and alarms (ADR 0113).
pub const LAB_UPKEEP: &str = "Home lab upkeep";

/// The most pieces of evidence a proposal names.
const MOST_EVIDENCE: usize = 20;

/// A project a lead or the CEO proposes, as its tool gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The goal it would serve.
    pub goal: GoalId,
    /// What it is.
    pub title: String,
    /// Why it is worth doing.
    pub reason: String,
    /// Who would lead it.
    pub lead: MemberId,
    /// The monthly cap asked for, if any: the owner's to grant.
    pub cap: Option<Micros>,
    /// The tasks it points at.
    pub tasks: Vec<TaskId>,
    /// The events (alarms, say) it points at.
    pub events: Vec<i64>,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- reading --------------------------------------------------------------------------

    pub(super) fn project(&self, id: ProjectId) -> Done<Project> {
        self.store
            .projects()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(EngineError::Unknown("project"))
    }

    /// The project a task is in, if any.
    pub(super) fn project_of(&self, task: Option<TaskId>) -> Done<Option<ProjectId>> {
        let Some(task) = task else {
            return Ok(None);
        };
        Ok(self.store.task(task)?.and_then(|t| t.project))
    }

    /// Whether the owner paused `project`: then no turn is taken on its work.
    pub(super) fn held_by_owner(&self, project: Option<ProjectId>) -> Done<bool> {
        let Some(project) = project else {
            return Ok(false);
        };
        let found = self.project(project)?;
        Ok(found.status == ProjectStatus::Paused && found.pause == Some(Pause::Owner))
    }

    /// The calendar month now, in UTC: from, until.
    fn month(&self) -> (Millis, Millis) {
        projects::month_around(self.clock.now())
    }

    fn spent_this_month(&self, project: ProjectId) -> Done<Micros> {
        let (from, until) = self.month();
        Ok(self.store.project_spent(project, from, until)?)
    }

    /// What a paid member's turn on `project`'s work may still reserve this month: `None` for
    /// no project or no cap, nothing while it is paused for its budget.
    pub(super) fn project_left(&self, project: Option<ProjectId>) -> Done<Option<Micros>> {
        let Some(project) = project else {
            return Ok(None);
        };
        let found = self.project(project)?;
        if found.pause == Some(Pause::Budget) {
            return Ok(Some(0));
        }
        let Some(cap) = found.cap else {
            return Ok(None);
        };
        let (from, until) = self.month();
        let committed = self.store.project_committed(project, from, until)?;
        Ok(Some(projects::left(committed, cap)))
    }

    /// The caps of every project that holds one against the pool's month, but `except`.
    fn caps_held(&self, except: Option<ProjectId>) -> Done<Micros> {
        Ok(self
            .store
            .projects()?
            .iter()
            .filter(|p| p.status.holds_its_cap() && Some(p.id) != except)
            .filter_map(|p| p.cap)
            .sum())
    }

    fn may_cap(&self, cap: Micros, project: Option<ProjectId>) -> Done<()> {
        projects::may_cap(cap, self.caps_held(project)?, self.settings.pool_per_month)
            .map_err(EngineError::Refused)
    }

    fn emit_project(&self, project: &Project, by: &str, what: &str) -> Done<()> {
        self.emit(
            EventKind::ProjectChanged,
            json!({ "project": project.id.0, "title": project.title, "goal": project.goal.0,
                    "status": project.status.name(), "pause": project.pause.map(Pause::name),
                    "cap": project.cap, "by": by, "what": what }),
        )
    }

    fn save_project(&self, project: &mut Project) -> Done<()> {
        project.updated_at = self.clock.now();
        Ok(self.store.save_project(project)?)
    }

    fn project_desk(
        &self,
        kind: &str,
        project: &Project,
        from: Option<MemberId>,
        text: &str,
    ) -> Done<crate::domain::DeskId> {
        let desk =
            self.store
                .add_desk(kind, None, Some(project.goal), from, text, self.clock.now())?;
        self.store.set_desk_project(desk, project.id)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": kind, "project": project.id.0 }),
        )?;
        Ok(desk)
    }

    /// Puts away what waits on the desk about a project, of `kind`, as `how`.
    fn put_away_desk(&self, project: ProjectId, kind: &str, how: &str) -> Done<()> {
        for item in self.store.desk()? {
            if item.project != Some(project) || item.kind != kind || item.status != "waiting" {
                continue;
            }
            self.store.set_desk_status(item.id, how)?;
            self.emit(
                EventKind::DeskDecided,
                json!({ "item": item.id.0, "kind": item.kind, "decision": how }),
            )?;
        }
        Ok(())
    }

    // ---- whom members report to -------------------------------------------------------------

    // ---- the owner's projects ------------------------------------------------------------

    /// The owner makes a project, active at once: under an active goal, led by a lead or the
    /// CEO, with a cap if they give one (all the caps together within the pool's month).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a goal or member that does not exist, a goal that is not
    /// active, or no title; [`Refusal::NotAProjectLead`]; [`Refusal::CapsOverPool`].
    pub fn add_project(
        &self,
        goal: GoalId,
        title: &str,
        reason: &str,
        lead: MemberId,
        cap: Option<Micros>,
    ) -> Done<ProjectId> {
        let _held = self.held();
        let (title, reason) = self.project_words(goal, title, reason, lead)?;
        if let Some(cap) = cap {
            self.may_cap(cap, None)?;
        }
        let id = self.store.add_project(
            &NewProject {
                goal,
                title,
                reason,
                lead,
                status: ProjectStatus::Active,
                cap,
                asked_cap: None,
                created_by: "owner".to_owned(),
                proposer: None,
                proposed_in: None,
                evidence: Vec::new(),
            },
            self.clock.now(),
        )?;
        self.emit_project(&self.project(id)?, "owner", "made")?;
        Ok(id)
    }

    /// A project's title and reason, each on one line, once its goal is active and its lead
    /// may lead.
    fn project_words(
        &self,
        goal: GoalId,
        title: &str,
        reason: &str,
        lead: MemberId,
    ) -> Done<(String, String)> {
        let found = self.store.goal(goal)?.ok_or(EngineError::Unknown("goal"))?;
        if found.status != GoalStatus::Active {
            return Err(EngineError::Unknown("active goal to serve"));
        }
        projects::may_lead(self.member(lead)?.role).map_err(EngineError::Refused)?;
        let title = one_line(title, 120);
        if title.is_empty() {
            return Err(EngineError::Unknown("project title"));
        }
        Ok((title, one_line(reason, 600)))
    }

    /// The owner, who is the board, changes a project: `pause` (every turn on it waits),
    /// `resume`, `drop` (its open work is dropped with it), `reopen` (a proposed, dropped or
    /// done project, active again: the board overrules), or `done` (once nothing in it is
    /// open). Returns where it is now.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project or an action there is not;
    /// [`Refusal::NotAProjectStep`] for a step it does not take from where it is;
    /// [`Refusal::CapNotAboveSpend`] to resume a project still past its cap.
    pub fn change_project(&self, id: ProjectId, action: &str) -> Done<ProjectStatus> {
        let _held = self.held();
        let mut project = self.project(id)?;
        let from = project.status;
        let refuse = || {
            Err(EngineError::Refused(Refusal::NotAProjectStep {
                from: from.name(),
                asked: action.to_owned(),
            }))
        };
        match action {
            "pause" => {
                if !matches!(from, ProjectStatus::Active | ProjectStatus::Paused) {
                    return refuse();
                }
                project.status = ProjectStatus::Paused;
                project.pause = Some(Pause::Owner);
            }
            "resume" => {
                if from != ProjectStatus::Paused {
                    return refuse();
                }
                if let Some(cap) = project.cap {
                    let spent = self.spent_this_month(id)?;
                    if level_spent(spent, cap) {
                        return Err(EngineError::Refused(Refusal::CapNotAboveSpend { spent }));
                    }
                }
                project.status = ProjectStatus::Active;
                project.pause = None;
                self.put_away_desk(id, "project_budget", "resumed")?;
            }
            "reopen" => {
                if !matches!(
                    from,
                    ProjectStatus::Proposed | ProjectStatus::Dropped | ProjectStatus::Done
                ) {
                    return refuse();
                }
                project.status = ProjectStatus::Active;
                project.pause = None;
                project.asked_cap = None;
            }
            "drop" | "done" => {
                if !from.holds_its_cap() {
                    return refuse();
                }
                let open: Vec<Task> = self
                    .store
                    .tasks()?
                    .into_iter()
                    .filter(|t| t.project == Some(id) && !t.status.finished())
                    .collect();
                if action == "done" && !open.is_empty() {
                    return Err(EngineError::Refused(Refusal::NotAProjectStep {
                        from: from.name(),
                        asked: format!("finish while {} of its tasks are open", open.len()),
                    }));
                }
                project.status = if action == "done" {
                    ProjectStatus::Done
                } else {
                    ProjectStatus::Dropped
                };
                project.pause = None;
                project.asked_cap = None;
                self.put_away(
                    &open,
                    &|item| item.project == Some(id),
                    "its project was dropped",
                )?;
                self.let_routines_go(id)?;
            }
            _ => return Err(EngineError::Unknown("action for a project")),
        }
        self.save_project(&mut project)?;
        self.emit_project(&project, "owner", action)?;
        Ok(project.status)
    }

    /// A project's routines go on under its goal, in no project, once it is over.
    fn let_routines_go(&self, project: ProjectId) -> Done<()> {
        for mut routine in self.store.routines()? {
            if routine.project != Some(project) {
                continue;
            }
            routine.project = None;
            self.store.save_routine(&routine)?;
        }
        Ok(())
    }

    /// Drops every task of `open`, ends their turns, and puts away what waits on the desk
    /// about them, or what `about` says is about the same thing, with the word the owner would
    /// have used to let it go.
    pub(super) fn put_away(
        &self,
        open: &[Task],
        about: &dyn Fn(&DeskItem) -> bool,
        why: &str,
    ) -> Done<()> {
        for task in open {
            self.store
                .set_task_status(task.id, crate::domain::TaskStatus::Dropped)?;
            self.emit_status(task, crate::domain::TaskStatus::Dropped)?;
            // What it was for is gone (ADR 0124): its goal retired, or its project closed.
            self.closed(
                task.id,
                crate::domain::closure::Closure::Dropped(crate::domain::closure::DropWhy::Obsolete),
                None,
                "endora",
            )?;
        }
        for task in open {
            self.after_closing(task.id)?;
        }
        let ids: Vec<TaskId> = open.iter().map(|t| t.id).collect();
        for run in self.store.live_runs()? {
            if !run.task.is_some_and(|t| ids.contains(&t)) {
                continue;
            }
            if run.status == RunStatus::Running {
                self.runner.stop(run.id, self.settings.grace);
            }
            self.end(run, RunStatus::Stopped, why)?;
        }
        for item in self.store.desk()? {
            let concerned = about(&item) || item.task.is_some_and(|t| ids.contains(&t));
            if item.status != "waiting" || !concerned {
                continue;
            }
            let decision = match item.kind.as_str() {
                "goal" | "project_cap" | "approval" => "decline",
                "stuck" => "drop",
                "ready_to_merge" | "ready_to_read" => "not_now",
                "project_budget" => "wait",
                _ => "dismiss",
            };
            self.store.set_desk_status(item.id, decision)?;
            self.emit(
                EventKind::DeskDecided,
                json!({ "item": item.id.0, "kind": item.kind, "decision": decision }),
            )?;
        }
        Ok(())
    }

    /// A retired goal's projects are dropped with it.
    pub(super) fn drop_projects_of(&self, goal: GoalId) -> Done<()> {
        for mut project in self.store.projects()? {
            if project.goal != goal || !project.status.holds_its_cap() {
                continue;
            }
            project.status = ProjectStatus::Dropped;
            project.pause = None;
            self.save_project(&mut project)?;
            self.emit_project(&project, "endora", "its goal was retired")?;
        }
        Ok(())
    }

    /// The owner sets a project's cap, raises it, or (`None`) takes it away. All the caps
    /// together stay within the pool's month; a project paused for its budget goes on once its
    /// cap is more than it spent this month.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project there is not; [`Refusal::CapsOverPool`];
    /// [`Refusal::CapNotAboveSpend`] for a project paused for its budget.
    pub fn set_project_cap(&self, id: ProjectId, cap: Option<Micros>) -> Done<()> {
        let _held = self.held();
        let mut project = self.project(id)?;
        self.cap_held_lock(&mut project, cap)?;
        self.put_away_desk(id, "project_cap", "set_on_screen")
    }

    fn cap_held_lock(&self, project: &mut Project, cap: Option<Micros>) -> Done<()> {
        if let Some(cap) = cap {
            self.may_cap(cap, Some(project.id))?;
        }
        let budget_paused = project.pause == Some(Pause::Budget);
        if let (Some(cap), true) = (cap, budget_paused) {
            projects::may_raise(cap, self.spent_this_month(project.id)?)
                .map_err(EngineError::Refused)?;
        }
        project.cap = cap;
        project.asked_cap = None;
        if budget_paused {
            project.status = ProjectStatus::Active;
            project.pause = None;
            self.put_away_desk(project.id, "project_budget", "raise")?;
        }
        self.save_project(project)?;
        self.emit_project(project, "owner", "capped")
    }

    /// The owner sets the tools a project's work is given (the Holodeck plan's phase 4), or
    /// (`None`) takes the list away: names trimmed, each once, in the owner's order.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project there is not; a store failure.
    pub fn set_project_tools(&self, id: ProjectId, tools: Option<Vec<String>>) -> Done<()> {
        let _held = self.held();
        let project = self.project(id)?;
        let tools = tools.map(|all| {
            let mut kept: Vec<String> = Vec::new();
            for name in all.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
                if !kept.iter().any(|k| k == name) {
                    kept.push(name.to_owned());
                }
            }
            kept
        });
        self.store
            .set_setting(&projects::tools_key(id), &json!(tools).to_string())?;
        self.emit(
            EventKind::ProjectChanged,
            json!({ "project": id.0, "title": project.title, "by": "owner", "what": "tools",
                    "tools": tools }),
        )
    }

    /// The tools a project's work is given, if the owner narrowed them.
    pub fn project_tools(&self, id: ProjectId) -> Done<Option<Vec<String>>> {
        Ok(projects::tools_read(
            self.store.setting(&projects::tools_key(id))?.as_deref(),
        ))
    }

    /// The project a turn's work is in and the tools it gives that work, if it narrows them.
    pub(super) fn project_tools_of_run(&self, run: RunId) -> Done<Option<(Project, Vec<String>)>> {
        let task = self.store.run(run)?.and_then(|r| r.task);
        let Some(id) = self.project_of(task)? else {
            return Ok(None);
        };
        let Some(tools) = self.project_tools(id)? else {
            return Ok(None);
        };
        Ok(Some((self.project(id)?, tools)))
    }

    /// Why a turn on a narrowed project's work is refused one of HQ's tools, if it is: any
    /// but its own four and what the project gives it.
    pub fn project_refuses(&self, run: RunId, tool: &str) -> Done<Option<String>> {
        let Some((project, tools)) = self.project_tools_of_run(run)? else {
            return Ok(None);
        };
        // HQ's own tools only come here; with `team` listed, the member keeps them all.
        if crate::domain::deploy::HQ_TOOLS.contains(&tool)
            || tools
                .iter()
                .any(|t| t == tool || t == crate::domain::projects::TEAM)
        {
            return Ok(None);
        }
        Ok(Some(format!(
            "{tool} is not among the tools of project \"{}\": its work is given {} and nothing else",
            project.title,
            crate::domain::deploy::HQ_TOOLS
                .iter()
                .map(|t| (*t).to_owned())
                .chain(tools)
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    /// The owner decides a cap the CEO asked for (`approve`, at what the owner writes or what
    /// was asked; `decline`), or a budget pause (`raise`, to what the owner writes; `wait` for
    /// the month to turn).
    pub(super) fn owner_decides_cap(
        &self,
        found: &DeskItem,
        decision: &str,
        words: &str,
    ) -> Done<()> {
        let mut project = self.project(found.project.ok_or(EngineError::Unknown("project"))?)?;
        let amount = projects::read_dollars(words);
        match (found.kind.as_str(), decision) {
            ("project_cap", "approve") => {
                let cap = amount
                    .or(project.asked_cap)
                    .ok_or(EngineError::Unknown("cap, in dollars"))?;
                self.cap_held_lock(&mut project, Some(cap))?;
            }
            ("project_cap", "decline") => {
                project.asked_cap = None;
                self.save_project(&mut project)?;
            }
            ("project_budget", "raise") => {
                let cap = amount.ok_or(EngineError::Unknown("new cap, in dollars"))?;
                self.cap_held_lock(&mut project, Some(cap))?;
            }
            ("project_budget", "wait") => {}
            _ => return Err(EngineError::Unknown("decision for this item")),
        }
        if self.waiting_on_desk(found.id).is_err() {
            return Ok(());
        }
        self.store.set_desk_status(found.id, decision)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": found.id.0, "kind": found.kind, "decision": decision }),
        )
    }

    // ---- money -----------------------------------------------------------------------------

    /// After a project's spend moved: at 80% of its cap, one note a month on the owner's desk;
    /// at all of it, a budget pause and the owner's choice on the desk. Nothing is blocked at
    /// 80%.
    pub(super) fn watch_budget(&self, project: Option<ProjectId>) -> Done<()> {
        let Some(id) = project else {
            return Ok(());
        };
        let mut project = self.project(id)?;
        let (Some(cap), ProjectStatus::Active) = (project.cap, project.status) else {
            return Ok(());
        };
        let spent = self.spent_this_month(id)?;
        match projects::level(spent, cap) {
            Level::Under => Ok(()),
            Level::Warned => {
                let key = format!(
                    "project_warned:{}:{}",
                    id.0,
                    projects::month_of(self.clock.now())
                );
                if self.store.setting(&key)?.is_some() {
                    return Ok(());
                }
                self.store.set_setting(&key, "1")?;
                let said = format!(
                    "has spent {}% of its {} cap this month ({})",
                    projects::WARN_PERCENT,
                    dollars(cap),
                    dollars(spent)
                );
                self.project_desk(
                    "notice",
                    &project,
                    None,
                    &format!("Project #{} {} {said}.", id.0, project.title),
                )?;
                self.emit(
                    EventKind::ProjectBudget,
                    json!({ "project": id.0, "title": project.title, "level": "warned",
                            "spent": spent, "cap": cap, "said": said }),
                )
            }
            Level::Spent => {
                project.status = ProjectStatus::Paused;
                project.pause = Some(Pause::Budget);
                self.save_project(&mut project)?;
                let said = format!(
                    "spent its {} cap for the month ({}): its Claude turns wait",
                    dollars(cap),
                    dollars(spent)
                );
                self.project_desk(
                    "project_budget",
                    &project,
                    None,
                    &format!(
                        "Project #{} {} {said}; members on the house's model go on.\nRaise the \
                         cap, or wait for next month: the pause lifts itself when the month \
                         turns.",
                        id.0, project.title
                    ),
                )?;
                self.emit(
                    EventKind::ProjectBudget,
                    json!({ "project": id.0, "title": project.title, "level": "spent",
                            "spent": spent, "cap": cap, "said": said }),
                )?;
                self.emit_project(&project, "endora", "paused for its budget")
            }
        }
    }

    /// A project paused for its budget goes on again once it is under its cap: the month
    /// turned, or its cap went. A pause the owner made stays until the owner lifts it.
    pub(super) fn lift_budget_pauses(&self) -> Done<()> {
        for mut project in self.store.projects()? {
            if project.status != ProjectStatus::Paused || project.pause != Some(Pause::Budget) {
                continue;
            }
            if let Some(cap) = project.cap {
                if level_spent(self.spent_this_month(project.id)?, cap) {
                    continue;
                }
            }
            project.status = ProjectStatus::Active;
            project.pause = None;
            self.save_project(&mut project)?;
            self.put_away_desk(project.id, "project_budget", "month_turned")?;
            self.emit_project(&project, "endora", "the month turned")?;
        }
        Ok(())
    }

    // ---- proposals -------------------------------------------------------------------------

    /// The evidence a proposal names, each piece checked to be in HQ's record.
    fn evidence(&self, tasks: &[TaskId], events: &[i64]) -> Done<Vec<Evidence>> {
        let mut all: Vec<Evidence> = tasks.iter().copied().map(Evidence::Task).collect();
        all.extend(events.iter().copied().map(Evidence::Event));
        all.dedup();
        if all.is_empty() {
            return Err(EngineError::Refused(Refusal::NoEvidence));
        }
        if all.len() > MOST_EVIDENCE {
            return Err(EngineError::Refused(Refusal::TooLong(MOST_EVIDENCE)));
        }
        for piece in &all {
            let there = match piece {
                Evidence::Task(task) => self.store.task(*task)?.is_some(),
                Evidence::Event(event) => self.store.has_event(*event)?,
            };
            if !there {
                return Err(EngineError::Refused(Refusal::NoSuchEvidence(piece.said())));
            }
        }
        Ok(all)
    }

    /// A turn of a lead's proposes a project, with evidence from HQ's record: it waits for the
    /// CEO, who decides (ADR 0120: leads propose, the CEO decides and proposes none). A cap
    /// asked for is the owner's to grant: until then the project has none of its own (the
    /// pool's and the members' limits still hold).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyLeadsPropose`]; [`Refusal::NotAProjectLead`];
    /// [`Refusal::NoEvidence`] or [`Refusal::NoSuchEvidence`]; [`Refusal::CapsOverPool`];
    /// [`EngineError::Unknown`] for a goal that is not active, a member there is not, or no
    /// title or reason.
    pub fn propose_project(&self, run: RunId, proposal: &Proposal) -> Done<ProjectId> {
        let _held = self.held();
        let turn = self.running(run)?;
        let member = self.member(turn.member)?;
        projects::may_propose(member.role).map_err(EngineError::Refused)?;
        let (title, reason) = self.project_words(
            proposal.goal,
            &proposal.title,
            &proposal.reason,
            proposal.lead,
        )?;
        if reason.is_empty() {
            return Err(EngineError::Unknown("reason for the project"));
        }
        let evidence = self.evidence(&proposal.tasks, &proposal.events)?;
        if let Some(cap) = proposal.cap {
            self.may_cap(cap, None)?;
        }
        let id = self.store.add_project(
            &NewProject {
                goal: proposal.goal,
                title,
                reason,
                lead: proposal.lead,
                status: ProjectStatus::Proposed,
                cap: None,
                asked_cap: proposal.cap,
                created_by: member.name.clone(),
                proposer: Some(member.id),
                proposed_in: turn.task,
                evidence,
            },
            self.clock.now(),
        )?;
        let project = self.project(id)?;
        self.emit_project(&project, &member.name, "proposed")?;
        if let Some(task) = turn.task {
            self.mark_progress(member.id, task)?;
        }
        Ok(id)
    }

    /// A proposed project starts: active, its lead handed a first task in it to plan its
    /// work, and a cap asked for put to the owner with the CEO's recommendation.
    fn start_project(&self, project: &mut Project, ceo: &Member, why: &str) -> Done<()> {
        let asked = project.asked_cap;
        project.status = ProjectStatus::Active;
        project.pause = None;
        self.save_project(project)?;
        self.emit_project(project, &ceo.name, "started")?;
        self.add_task_held_lock(
            Author::Endora,
            NewTask {
                goal: project.goal,
                project: Some(project.id),
                parent: None,
                title: project.title.clone(),
                description: format!(
                    "{}\n\nThis is the first task of project #{}, which you lead: plan its work \
                     and hand its parts on with create_task. Its tasks stay in the project.",
                    project.reason, project.id
                ),
                assignee: Some(project.lead),
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )?;
        let Some(cap) = asked else {
            return Ok(());
        };
        self.project_desk(
            "project_cap",
            project,
            Some(ceo.id),
            &format!(
                "A cap for project #{} {}: {} a month.\nThe CEO recommends: {}",
                project.id,
                project.title,
                dollars(cap),
                one_line(why, 600)
            ),
        )?;
        Ok(())
    }

    /// A turn of the CEO's decides a proposed project: `accept` starts it (a cap asked for,
    /// or the one the CEO gives, goes to the owner with the CEO's `why` as its recommendation),
    /// or it is sent back, dropped. Either way the CEO's word is posted in the thread the
    /// proposal was made in, and its proposer woken to read it.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::OnlyTheCeo`]; [`EngineError::Unknown`] for a project
    /// that is not waiting for the CEO, or no why; [`Refusal::CapsOverPool`].
    pub fn decide_project(
        &self,
        run: RunId,
        id: ProjectId,
        accept: bool,
        why: &str,
        cap: Option<Micros>,
        cited: &[crate::domain::Ref],
    ) -> Done<ProjectStatus> {
        let _held = self.held();
        let (turn, ceo) = self.the_ceos_turn(run)?;
        // The CEO decides on evidence from HQ's record, as a proposal is made on it (ADR 0114).
        self.judge_decision(cited, turn.id)?;
        let why = if cited.is_empty() {
            why.to_owned()
        } else {
            format!("{} (evidence: {})", why.trim(), self.evidence_said(cited)?)
        };
        let why = why.as_str();
        let mut project = self.project(id)?;
        if project.status != ProjectStatus::Proposed {
            return Err(EngineError::Unknown("proposed project with that number"));
        }
        let why = one_line(why, 600);
        if why.is_empty() {
            return Err(EngineError::Unknown("why"));
        }
        let word = if accept {
            project.asked_cap = cap.or(project.asked_cap);
            if let Some(cap) = project.asked_cap {
                self.may_cap(cap, Some(id))?;
            }
            self.start_project(&mut project, &ceo, &why)?;
            "accepted"
        } else {
            project.status = ProjectStatus::Dropped;
            project.asked_cap = None;
            self.save_project(&mut project)?;
            self.emit_project(&project, &ceo.name, "sent back")?;
            "sent back"
        };
        if let Some(task) = turn.task {
            self.mark_progress(ceo.id, task)?;
        }
        let open = match project.proposed_in {
            Some(task) => Some(self.task(task)?).filter(|t| !t.status.finished()),
            None => None,
        };
        if let Some(task) = open {
            self.post_held_lock(
                Author::Member(ceo.id),
                task.id,
                &format!("Project #{} {}: {word}. {why}", id.0, project.title),
            )?;
            if let Some(proposer) = project.proposer.filter(|p| *p != ceo.id) {
                self.wake_held_lock(proposer, Some(task.id), WakeReason::ProposalDecided)?;
            }
        }
        Ok(project.status)
    }

    // ---- at start -------------------------------------------------------------------------

    /// Once, on a store made before projects: the home lab's routines and their open work go
    /// in one active project, "Home lab upkeep", under the goal of the routine that takes the
    /// alarms, led by the earliest lead at work (or the CEO), with no cap. The CEO's own
    /// routines (its daily review) stay out of it: they look over the whole house. Nothing once
    /// any project exists, or with no routine that takes the alarms. Returns the project made.
    pub fn migrate_projects(&self) -> Done<Option<ProjectId>> {
        let _held = self.held();
        if !self.store.projects()?.is_empty() {
            return Ok(None);
        }
        let routines = self.store.routines()?;
        let Some(goal) = routines.iter().find(|r| r.on_lab_trouble).map(|r| r.goal) else {
            return Ok(None);
        };
        let members = self.store.members()?;
        let first = |role: Role| {
            members
                .iter()
                .filter(|m| m.role == role && !m.paused)
                .map(|m| m.id)
                .min()
        };
        let Some(lead) = first(Role::Lead).or_else(|| first(Role::Ceo)) else {
            return Ok(None);
        };
        let id = self.store.add_project(
            &NewProject {
                goal,
                title: LAB_UPKEEP.to_owned(),
                reason: "The home lab's routine checks and alarms, and the work they bring."
                    .to_owned(),
                lead,
                status: ProjectStatus::Active,
                cap: None,
                asked_cap: None,
                created_by: "endora".to_owned(),
                proposer: None,
                proposed_in: None,
                evidence: Vec::new(),
            },
            self.clock.now(),
        )?;
        let the_ceos = |id: MemberId| members.iter().any(|m| m.id == id && m.role == Role::Ceo);
        for mut routine in routines {
            if routine.goal != goal || the_ceos(routine.assignee) {
                continue;
            }
            routine.project = Some(id);
            self.store.save_routine(&routine)?;
        }
        for task in self.store.tasks()? {
            if task.goal == goal && task.project.is_none() && !task.status.finished() {
                self.store.set_task_project(task.id, Some(id))?;
            }
        }
        self.emit_project(&self.project(id)?, "endora", "made for the home lab")?;
        Ok(Some(id))
    }

    /// The owner puts an open task into an active project: its lead, tools and money are then
    /// the task's. It starts again there: off the ladder, blocked work back to todo, and its
    /// member woken. Live, #676 and #677 asked for the ansible-nas fork's secrets with no
    /// project, so their members had none of the fork's tools.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a task or project there is not; [`Refusal::NotAProjectStep`]
    /// for a project that is not active or a task that is finished.
    pub fn move_task_to_project(&self, task: TaskId, to: ProjectId) -> Done<()> {
        let _held = self.held();
        let found = self.task(task)?;
        let project = self.project(to)?;
        if project.status != ProjectStatus::Active || found.status.finished() {
            return Err(EngineError::Refused(Refusal::NotAProjectStep {
                from: project.status.name(),
                asked: "move a task in".to_owned(),
            }));
        }
        self.store.set_task_project(task, Some(to))?;
        if found.status == crate::domain::TaskStatus::Blocked {
            self.store
                .set_task_status(task, crate::domain::TaskStatus::Todo)?;
            self.emit_status(&found, crate::domain::TaskStatus::Todo)?;
        }
        let members = self.store.members()?;
        let lead = members
            .iter()
            .find(|m| m.id == project.lead)
            .map_or("nobody", |m| m.name.as_str());
        self.post_held_lock(
            Author::Owner,
            task,
            &format!(
                "Moved into the project {}, led by {lead}: its tools and budget are this \
                 task's now. Take it up again from here.",
                project.title
            ),
        )?;
        if let Some(member) = found.owner.or(found.assignee) {
            self.wake_held_lock(member, Some(task), WakeReason::Assigned)?;
        }
        Ok(())
    }

    // ---- what members and the screen are told ----------------------------------------------

    /// The projects as the screen shows them, with this month's money.
    pub(super) fn projects_shown(&self, tasks: &[Task]) -> Done<serde_json::Value> {
        let (from, until) = self.month();
        let mut shown = Vec::new();
        for p in self.store.projects()? {
            let spent = self.store.project_spent(p.id, from, until)?;
            shown.push(json!({
                "id": p.id.0, "goal": p.goal.0, "title": p.title, "reason": p.reason,
                "lead": p.lead.0, "status": p.status.name(), "pause": p.pause.map(Pause::name),
                "cap": p.cap, "asked_cap": p.asked_cap, "spent": spent,
                "committed": self.store.project_committed(p.id, from, until)?,
                "level": p.cap.map(|cap| match projects::level(spent, cap) {
                    Level::Under => "under",
                    Level::Warned => "warned",
                    Level::Spent => "spent",
                }),
                "created_by": p.created_by, "proposer": p.proposer.map(|m| m.0),
                "tools": self.project_tools(p.id)?,
                "evidence": p.evidence.iter().map(|e| e.said()).collect::<Vec<_>>(),
                "open_tasks": tasks.iter()
                    .filter(|t| t.project == Some(p.id) && !t.status.finished()).count(),
            }));
        }
        Ok(serde_json::Value::Array(shown))
    }
}

/// Whether `spent` is all of `cap`, as a budget pause judges it.
const fn level_spent(spent: Micros, cap: Micros) -> bool {
    matches!(projects::level(spent, cap), Level::Spent)
}
