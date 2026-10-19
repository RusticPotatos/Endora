//! Process health (ADR 0124): detectors over run labels and the record, per project and per
//! piece of work outside one, that catch a team talking about work instead of doing it, a task
//! bounced between its worker and its checkers, and a member woken again and again for nothing.
//! Each fires a finding once a key, and each finding takes a verdict from the owner or the lead
//! of its area, so every guard's precision is measured and its threshold can be tuned.

use serde_json::{Value, json};

use super::guards::guard_card;
use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::application::scorecard::Window;
use crate::application::scorecard::outcomes::talk_work;
use crate::domain::evidence::Ref;
use crate::domain::guards::{self, Ceremony, Guard, THRESHOLDS, TurnKind, TurnSeen, Verdict};
use crate::domain::ladder::Rung;
use crate::domain::{
    EventKind, MemberId, Millis, ProjectStatus, Refusal, RunId, Task, TaskId, WakeReason,
};

/// Where HQ keeps when it last looked for process-health findings.
const LOOKED_AT: &str = "guards_looked_at";
/// Where HQ keeps each project's ceremony as it last looked: the scorecard's line.
const CEREMONY_SEEN: &str = "guards_ceremony";
/// Where HQ keeps the whole team's talk against work as it last looked, for the outcomes line
/// (ADR 0132): the same labels as the ceremony, over the same day and week.
pub(super) const TALK_SEEN: &str = "guards_talk";

/// One ended turn, as the process-health look reads it.
struct Seen {
    /// The project of its task, if the task is in one.
    project: Option<i64>,
    /// The task it was on.
    task: TaskId,
    /// The top of the task tree that task hangs from; the task itself when it is the top (ADR
    /// 0133).
    root: TaskId,
    /// What it did.
    turn: TurnSeen,
    /// When each act of its that succeeded was done.
    acts: Vec<Millis>,
}

/// What a ceremony is the ceremony of: a project, or, for work outside any, the task tree it is
/// kept under (ADR 0133).
struct Subject {
    /// The key its finding is kept under: one finding for a guard and what it is about.
    key: String,
    /// What the card and the finding call it.
    title: String,
    /// Its project, if it is a project's.
    project: Option<i64>,
    /// Its lead, if it is a project's; a tree's is its busiest task's area's lead.
    lead: Option<MemberId>,
}

/// The top of the task tree `task` hangs from. A tree is a few deep at most, and a cycle in the
/// record (there should be none) ends the walk where it began.
fn root_of(tasks: &[Task], task: TaskId) -> TaskId {
    let mut at = task;
    for _ in 0..tasks.len() {
        let Some(up) = tasks.iter().find(|t| t.id == at).and_then(|t| t.parent) else {
            return at;
        };
        at = up;
    }
    at
}

/// The wakes that ask for a review round, or a huddle's view: coordination by their nature.
const CHECKING: [WakeReason; 3] = [
    WakeReason::ReviewRequested,
    WakeReason::AcceptanceAsked,
    WakeReason::Huddle,
];
/// How many findings the screen shows.
const FINDINGS_SHOWN: usize = 30;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the tick's look ----------------------------------------------------------------------

    /// The process-health part of the tick, at most every [`THRESHOLDS`]`.look_every_ms`:
    /// each project's ceremony over the day and the week, and findings that cleared.
    pub(super) fn health_tick(&self) -> Done<()> {
        self.clear_what_moved()?;
        let now = self.clock.now();
        let looked: Millis = self
            .store
            .setting(LOOKED_AT)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(Millis::MIN);
        if now.saturating_sub(looked) < THRESHOLDS.look_every_ms {
            return Ok(());
        }
        self.store.set_setting(LOOKED_AT, &now.to_string())?;
        let turns = self.turns_seen(now)?;
        self.look_for_ceremony(now, &turns)?;
        self.look_at_talk(now, &turns)
    }

    /// Carousel and redundant-wake findings whose work moved on clear themselves.
    fn clear_what_moved(&self) -> Done<()> {
        for finding in self.store.findings()? {
            if finding.cleared_at.is_some() {
                continue;
            }
            // A fence push clears once a day has passed since it was found: what it counted is
            // out of its window.
            if finding.guard == Guard::FencePush {
                if self.clock.now() - finding.opened_at >= THRESHOLDS.fence_window_ms {
                    self.clear(&finding)?;
                }
                continue;
            }
            // The same ask clears once the owner answered its card.
            if finding.guard == Guard::SameAsk {
                let desk: Option<i64> = finding.key.split(':').nth(1).and_then(|d| d.parse().ok());
                let answered = self
                    .store
                    .desk()?
                    .iter()
                    .any(|d| Some(d.id.0) == desk && d.status != "waiting");
                if answered {
                    self.clear(&finding)?;
                }
                continue;
            }
            let Some(task) = finding.task.map(TaskId) else {
                continue;
            };
            let over = self.store.task(task)?.is_none_or(|t| t.status.finished());
            let cleared = match finding.guard {
                Guard::Carousel => over || matches!(self.rung_of(task)?, Some((Rung::Decided, _))),
                Guard::RedundantWake => {
                    let member = finding
                        .key
                        .rsplit(':')
                        .next()
                        .and_then(|m| m.parse().ok())
                        .map(MemberId);
                    over || member.is_none_or(|m| !self.woken_for_nothing(m, task).unwrap_or(false))
                }
                _ => false,
            };
            if cleared {
                self.clear(&finding)?;
            }
        }
        Ok(())
    }

    /// What each ended turn of the last week did, for the ceremony detector and the outcomes
    /// line, with the project of its task if it has one.
    fn turns_seen(&self, now: Millis) -> Done<Vec<Seen>> {
        let tasks = self.store.tasks()?;
        let wakes = self.store.wakes()?;
        let branches = self.store.branches()?;
        let desk = self.store.desk()?;
        let week = guards::WINDOWS[1].0;
        let mut runs: Vec<Run> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.started_at.is_some() && r.label.is_some())
            .collect();
        runs.sort_by_key(|r| r.id);
        let mut seen_readings: Vec<(TaskId, String, String)> = Vec::new();
        let mut turns = Vec::new();
        for run in &runs {
            let (Some(task), Some(label), Some(ended)) = (run.task, run.label, run.ended_at) else {
                continue;
            };
            let receipts = self.store.call_receipts_of(run.id)?;
            let readings: Vec<(TaskId, String, String)> = receipts
                .iter()
                .filter(|r| r.ok && !r.act)
                .map(|r| {
                    (
                        task,
                        r.tool.clone(),
                        crate::domain::evidence::same_args(&r.args),
                    )
                })
                .collect();
            let new_evidence = readings.iter().any(|r| !seen_readings.contains(r));
            seen_readings.extend(readings);
            if now - ended >= week {
                continue;
            }
            let project = tasks
                .iter()
                .find(|t| t.id == task)
                .and_then(|t| t.project)
                .map(|p| p.0);
            let reason = wakes.iter().find(|w| w.id == run.wake).map(|w| w.reason);
            let started = run.started_at.unwrap_or(ended);
            let asked =
                desk.iter().any(|d| {
                    d.member == Some(run.member)
                        && d.task == Some(task)
                        && started <= d.at
                        && d.at <= ended
                }) || self.store.thread(task)?.iter().any(|p| {
                    p.text.starts_with("Question for ") && started <= p.at && p.at <= ended
                });
            turns.push(Seen {
                project,
                task,
                root: root_of(&tasks, task),
                turn: TurnSeen {
                    ended_at: ended,
                    label,
                    checking: reason.is_some_and(|r| CHECKING.contains(&r)),
                    acted: receipts.iter().any(|r| r.ok && r.act),
                    branch: branches.iter().any(|b| b.run == run.id),
                    criterion_met: self.store.cited(task)?.iter().any(|c| c.run == run.id),
                    new_evidence,
                    asked,
                    spent: run.spent,
                },
                acts: receipts
                    .iter()
                    .filter(|r| r.ok && r.act)
                    .map(|r| r.at)
                    .collect(),
            });
        }
        Ok(turns)
    }

    /// The whole team's talk against work over the day and the week, with the ceremony guard's
    /// own labels, kept for the outcomes line (ADR 0132): turns that coordinated against turns
    /// that changed the product, the posts members wrote and the acts that succeeded. A lead's
    /// report and patterns turns are the lead's standing reviews, not talk about work.
    fn look_at_talk(&self, now: Millis, turns: &[Seen]) -> Done<()> {
        let reports = self.report_task_ids()?;
        let mine: Vec<&Seen> = turns
            .iter()
            .filter(|s| !reports.contains(&s.task))
            .collect();
        let seen: Vec<TurnSeen> = mine.iter().map(|s| s.turn).collect();
        let acts: Vec<Millis> = mine.iter().flat_map(|s| s.acts.iter().copied()).collect();
        let week = guards::WINDOWS[1].0;
        let members: std::collections::HashSet<String> =
            self.store.members()?.into_iter().map(|m| m.name).collect();
        let mut posts = Vec::new();
        for task in self.store.tasks()? {
            if reports.contains(&task.id) {
                continue;
            }
            posts.extend(
                self.store
                    .thread(task.id)?
                    .into_iter()
                    .filter(|p| members.contains(&p.author) && now - p.at < week)
                    .map(|p| p.at),
            );
        }
        let over = |ms: Millis| talk_work(&seen, &posts, &acts, &Window::rolling(now, ms)).json();
        let kept = json!({ "at": now, "day": over(guards::WINDOWS[0].0), "week": over(week) });
        self.store.set_setting(TALK_SEEN, &kept.to_string())?;
        Ok(())
    }

    /// Each open project's ceremony over the day and the week (ADR 0124), and each piece of work
    /// outside a project, by the task tree it hangs from (ADR 0133): over the line, a finding, a
    /// notice on the owner's digest and the lead woken on the busiest task; back under it in both,
    /// the finding clears. Kept for the scorecard either way, for a project; a tree is on the
    /// card only while it is over.
    fn look_for_ceremony(&self, now: Millis, turns: &[Seen]) -> Done<()> {
        let mut shown = Vec::new();
        let mut looked_at = Vec::new();
        for project in self.store.projects()? {
            if matches!(project.status, ProjectStatus::Done | ProjectStatus::Dropped) {
                continue;
            }
            let mine: Vec<&Seen> = turns
                .iter()
                .filter(|s| s.project == Some(project.id.0))
                .collect();
            let subject = Subject {
                key: format!("ceremony:project:{}", project.id.0),
                title: project.title.clone(),
                project: Some(project.id.0),
                lead: Some(project.lead),
            };
            looked_at.push(subject.key.clone());
            shown.push(self.look_at_ceremony(now, &subject, &mine)?);
        }
        // Work outside any project (the owner's, a routine's) belongs to its task tree, the unit
        // its lead can act on: the top task and everything under it. A lead's standing reviews
        // are not talk about work.
        let reports = self.report_task_ids()?;
        let tasks = self.store.tasks()?;
        let loose = |s: &&Seen| s.project.is_none() && !reports.contains(&s.task);
        let mut roots: Vec<TaskId> = Vec::new();
        for seen in turns.iter().filter(loose) {
            if !roots.contains(&seen.root) {
                roots.push(seen.root);
            }
        }
        for root in roots {
            // Work that is all over has nothing left for its lead to decide.
            let open = tasks
                .iter()
                .any(|t| !t.status.finished() && root_of(&tasks, t.id) == root);
            if !open {
                continue;
            }
            let mine: Vec<&Seen> = turns
                .iter()
                .filter(loose)
                .filter(|s| s.root == root)
                .collect();
            let title = self.store.task(root)?.map_or_else(
                || format!("#{}", root.0),
                |t| format!("#{} {}", root.0, t.title),
            );
            let subject = Subject {
                key: format!("ceremony:tree:{}", root.0),
                title,
                project: None,
                lead: None,
            };
            looked_at.push(subject.key.clone());
            let entry = self.look_at_ceremony(now, &subject, &mine)?;
            if entry["over"] == true {
                shown.push(entry);
            }
        }
        // A tree whose turns have all aged out of the week has nothing left to be found on.
        for open in self.store.findings()? {
            if open.guard == Guard::Ceremony
                && open.cleared_at.is_none()
                && open.key.starts_with("ceremony:tree:")
                && !looked_at.contains(&open.key)
            {
                self.clear(&open)?;
            }
        }
        self.store
            .set_setting(CEREMONY_SEEN, &Value::Array(shown).to_string())?;
        Ok(())
    }

    /// One ceremony's look: its turns over the day and the week, what they spent, and, over the
    /// line, its finding and the lead it wakes. Returns what the Guards card shows of it.
    fn look_at_ceremony(&self, now: Millis, subject: &Subject, mine: &[&Seen]) -> Done<Value> {
        let seen: Vec<TurnSeen> = mine.iter().map(|s| s.turn).collect();
        let windows: Vec<(Ceremony, &str)> = guards::WINDOWS
            .iter()
            .map(|(ms, name)| (guards::ceremony(&seen, now, *ms), *name))
            .collect();
        let over = windows.iter().any(|(c, _)| c.over());
        let entry = json!({
            "project": subject.project, "title": subject.title, "over": over,
            "windows": windows.iter().map(|(c, name)| json!({
                "window": name, "coordination": c.coordination, "product": c.product,
                "over": c.over(), "said": c.said(name), "spent": c.spent })).collect::<Vec<_>>(),
        });
        if !over {
            if let Some(open) = self
                .store
                .findings()?
                .into_iter()
                .find(|f| f.key == subject.key && f.cleared_at.is_none())
            {
                self.clear(&open)?;
            }
            return Ok(entry);
        }
        let said: Vec<String> = windows
            .iter()
            .map(|(c, name)| c.said_with_cost(name))
            .collect();
        let evidence = format!("{}: {}", subject.title, said.join("; "));
        let busiest = mine
            .iter()
            .filter(|s| guards::turn_kind(&s.turn) == TurnKind::Coordination)
            .map(|s| s.task)
            .fold(Vec::<(TaskId, usize)>::new(), |mut acc, task| {
                match acc.iter_mut().find(|(t, _)| *t == task) {
                    Some(found) => found.1 += 1,
                    None => acc.push((task, 1)),
                }
                acc
            })
            .into_iter()
            .filter(|(task, _)| {
                self.store
                    .task(*task)
                    .ok()
                    .flatten()
                    .is_some_and(|t| !t.status.finished())
            })
            .max_by_key(|(_, n)| *n)
            .map(|(task, _)| task);
        // What it rests on is the week's talk, which contains the day's: a finding the owner
        // judged is raised again once a dozen more turns of it have been had (ADR 0132).
        let mark = i64::try_from(windows[1].0.coordination).unwrap_or(i64::MAX);
        let Some(id) = self.find_on_the_desk(
            Guard::Ceremony,
            &subject.key,
            busiest,
            subject.project,
            &evidence,
            mark,
        )?
        else {
            return Ok(entry);
        };
        let Some(task) = busiest else {
            return Ok(entry);
        };
        // A project's lead is woken for its project; work outside one goes to the lead of its
        // area, and where nobody leads it there is nobody to wake.
        let lead = match subject.lead {
            Some(lead) => Some(self.member(lead)?).filter(|m| m.at_work()),
            None => self.area_lead(&self.task(task)?)?,
        };
        let Some(lead) = lead else {
            return Ok(entry);
        };
        let whose = if subject.project.is_some() {
            "your project"
        } else {
            "in your area"
        };
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "Talking about work isn't work (finding {id}): {evidence}. {}, this is {whose}: \
                 decide what moves it on evidence. You may narrow its criteria (narrow_criteria), \
                 merge its tasks, drop what is not needed, or carry on and say why. Then say \
                 whether the finding was real with judge_finding.",
                lead.name
            ),
        )?;
        self.wake_held_lock(lead.id, Some(task), WakeReason::Guard)?;
        Ok(entry)
    }

    /// A finding as a notice on the owner's desk (in the digest, ADR 0132): the rule and what it
    /// fired on. Returned as its id when it is news, a finding that opened or opened again; a
    /// finding seen again is refreshed, its notice with it, and a judged one is left alone.
    pub(super) fn find_on_the_desk(
        &self,
        guard: Guard,
        key: &str,
        task: Option<TaskId>,
        project: Option<i64>,
        evidence: &str,
        mark: i64,
    ) -> Done<Option<i64>> {
        let found = self.find(guard, key, task, project, evidence, mark)?;
        if !found.how.is_news() {
            return Ok(None);
        }
        let id = found.id;
        let desk = self.store.add_desk(
            "guard",
            task,
            None,
            None,
            &guard_card(guard, id, evidence),
            self.clock.now(),
        )?;
        self.store.set_finding_desk(id, desk.0)?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": desk.0, "kind": "guard", "task": task.map(|t| t.0) }),
        )?;
        Ok(Some(id))
    }

    // ---- the review carousel ------------------------------------------------------------------

    /// Work just sent back: on the carousel (returned three times with no new evidence
    /// between), it goes straight to a huddle of its area (ADR 0124), not another round.
    pub(super) fn after_sent_back(&self, task: TaskId) -> Done<()> {
        let members = self.store.members()?;
        let returns: Vec<Millis> = self
            .store
            .thread(task)?
            .iter()
            .filter(|p| {
                p.text.starts_with(super::evidence::SENT_BACK)
                    && members.iter().any(|m| m.name == p.author)
            })
            .map(|p| p.at)
            .collect();
        let mut submitted: Vec<(RunId, Millis, Vec<Ref>)> = Vec::new();
        for c in self.store.cited(task)? {
            if c.stage != "submitted" {
                continue;
            }
            match submitted.iter_mut().find(|(run, _, _)| *run == c.run) {
                Some(found) => found.2.extend(c.refs),
                None => submitted.push((c.run, c.at, c.refs)),
            }
        }
        let submitted: Vec<(Millis, Vec<Ref>)> = submitted
            .into_iter()
            .map(|(_, at, refs)| (at, refs))
            .collect();
        if !guards::carousel(&returns, &submitted) {
            return Ok(());
        }
        if matches!(self.rung_of(task)?, Some((Rung::Huddle, _))) {
            return Ok(());
        }
        let current = self.task(task)?;
        let why = format!(
            "sent back {} times with no new evidence between",
            returns.len()
        );
        // It rests on how many times the work has been sent back (ADR 0132).
        self.find(
            Guard::Carousel,
            &format!("carousel:{}", task.0),
            Some(task),
            current.project.map(|p| p.0),
            &format!("#{} {}: {why}", task.0, current.title),
            i64::try_from(returns.len()).unwrap_or(i64::MAX),
        )?;
        let Some(lead) = self.area_lead(&current)? else {
            return Ok(());
        };
        self.call_huddle(&current, &lead, &why)
    }

    // ---- the CEO's rescues --------------------------------------------------------------------

    /// How often the chain of command's top reached into each member's work (ADR 0124): the
    /// CEO's directions on a task the member holds, and stuck work of the member's that climbed
    /// to the CEO. A member whose work is rescued again and again needs its brain, notebook or
    /// role looked at, not more rescuing. Counted by whoever holds each task now.
    pub(super) fn rescues(&self) -> Done<std::collections::HashMap<i64, usize>> {
        let tasks = self.store.tasks()?;
        let holder = |task: Option<i64>| {
            task.and_then(|t| tasks.iter().find(|x| x.id.0 == t))
                .and_then(|t| t.owner.or(t.assignee))
        };
        let mut count = std::collections::HashMap::new();
        for w in self.store.wakes()? {
            if w.reason != WakeReason::Directed {
                continue;
            }
            if let Some(m) = holder(w.task.map(|t| t.0)) {
                *count.entry(m.0).or_insert(0) += 1;
            }
        }
        for e in self.store.events_after(0)? {
            if e.kind != EventKind::StuckClimbed.name() || e.data["rung"] != "ceo" {
                continue;
            }
            if let Some(m) = holder(e.data["task"].as_i64()) {
                *count.entry(m.0).or_insert(0) += 1;
            }
        }
        Ok(count)
    }

    // ---- the same ask, once on the desk -------------------------------------------------------

    /// A question or a decision for the owner goes on the desk, unless the same ask from
    /// another task already waits there (ADR 0124): then it is put with that card ("also on
    /// #672"), said in its own thread, and counted as a finding. Live, the CEO put the same
    /// board decision on the desk twice, once a task, both checks having exited 127.
    pub(super) fn ask_on_the_desk(
        &self,
        kind: &str,
        task: TaskId,
        member: MemberId,
        text: &str,
    ) -> Done<crate::domain::DeskId> {
        let print = crate::domain::retraction::fingerprint(text);
        let waiting = self.store.desk()?.into_iter().find(|d| {
            d.kind == kind
                && d.status == "waiting"
                && d.task.is_some_and(|t| t != task)
                && guards::same_ask(&crate::domain::retraction::fingerprint(&d.text), &print)
        });
        let Some(open) = waiting else {
            let desk = self.store.add_desk(
                kind,
                Some(task),
                None,
                Some(member),
                text,
                self.clock.now(),
            )?;
            self.emit(
                EventKind::DeskAdded,
                json!({ "item": desk.0, "kind": kind, "task": task.0 }),
            )?;
            return Ok(desk);
        };
        let main = open.task.map_or(0, |t| t.0);
        self.store.attach_to_desk(open.id.0, task, Some(member))?;
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "The same ask as #{main}'s is already on the owner's desk: it is put with it, \
                 and the owner's one answer comes here too."
            ),
        )?;
        let project = self.task(task)?.project.map(|p| p.0);
        // One finding for each task put with a card; it rests on nothing that grows.
        self.find(
            Guard::SameAsk,
            &format!("same_ask:{}:{}", open.id.0, task.0),
            Some(task),
            project,
            &format!(
                "#{}'s {kind} is the same ask as #{main}'s, on the desk as item {}",
                task.0, open.id.0
            ),
            0,
        )?;
        Ok(open.id)
    }

    /// The owner's one answer to an ask, delivered to every task put with it (ADR 0124): in
    /// each one's thread, as the owner's, and whoever asked there woken to read it.
    pub(super) fn answer_the_same_asks(
        &self,
        item: crate::domain::DeskId,
        decision: &str,
        words: &str,
    ) -> Done<()> {
        let attached = self.store.attached(item.0)?;
        if attached.is_empty() {
            return Ok(());
        }
        let Some(found) = self.store.desk()?.into_iter().find(|d| d.id == item) else {
            return Ok(());
        };
        let main = found.task.map_or(0, |t| t.0);
        let said = match (decision, words.trim()) {
            ("approve", "") => "approved".to_owned(),
            ("decline", "") => "declined".to_owned(),
            ("approve", w) => format!("approved: {w}"),
            ("decline", w) => format!("declined: {w}"),
            (_, "") => "seen".to_owned(),
            (_, w) => format!("answered: {w}"),
        };
        for (task, member) in attached {
            self.post_held_lock(
                Author::Owner,
                task,
                &format!("The owner {said} (the same ask, put with #{main}'s)"),
            )?;
            let Some(to) = member.or(self.task(task)?.owner) else {
                continue;
            };
            self.wake_held_lock(to, Some(task), WakeReason::ProposalDecided)?;
        }
        Ok(())
    }

    // ---- pushing at the fence -----------------------------------------------------------------

    /// A refused act was just recorded in `run` (ADR 0124): three in one turn, or the same act
    /// refused for two members within a day, is a finding and a notice on the owner's desk
    /// naming the tool and the members. A notice, not a tripwire: the fence held; what the
    /// owner learns is what the team believes it should be doing.
    pub(super) fn after_refused_act(&self, run: RunId, tool: &str) -> Done<()> {
        let t = THRESHOLDS;
        let now = self.clock.now();
        let members = self.store.members()?;
        let name = |id: MemberId| {
            members
                .iter()
                .find(|m| m.id == id)
                .map_or_else(|| "someone".to_owned(), |m| m.name.clone())
        };
        let mine: Vec<_> = self
            .store
            .call_receipts_of(run)?
            .into_iter()
            .filter(|r| r.act && !r.ok)
            .collect();
        let task = mine.first().and_then(|r| r.task);
        let project = match task {
            Some(task) => self.store.task(task)?.and_then(|t| t.project).map(|p| p.0),
            None => None,
        };
        if mine.len() >= t.fence_refusals_in_a_turn {
            let who = mine.first().map(|r| name(r.member)).unwrap_or_default();
            let mut tools: Vec<String> = mine.iter().map(|r| r.tool.clone()).collect();
            tools.dedup();
            // A refusal rests on the newest refused act it counts: a later one is new (ADR 0132).
            let newest = mine.iter().map(|r| r.id).max().unwrap_or(0);
            self.find_on_the_desk(
                Guard::FencePush,
                &format!("fence:run:{}", run.0),
                task,
                project,
                &format!(
                    "{who} was refused {} acts in one turn: {}",
                    mine.len(),
                    tools.join(", ")
                ),
                newest,
            )?;
        }
        let mut who: Vec<MemberId> = Vec::new();
        let mut newest = 0;
        for r in self.store.runs()? {
            if r.started_at.is_none_or(|s| now - s >= t.fence_window_ms) {
                continue;
            }
            for receipt in self.store.call_receipts_of(r.id)? {
                if receipt.act && !receipt.ok && receipt.tool == tool {
                    newest = newest.max(receipt.id);
                    if !who.contains(&receipt.member) {
                        who.push(receipt.member);
                    }
                }
            }
        }
        if who.len() >= t.fence_members {
            let named: Vec<String> = who.iter().map(|m| name(*m)).collect();
            self.find_on_the_desk(
                Guard::FencePush,
                &format!("fence:tool:{tool}"),
                task,
                project,
                &format!(
                    "{tool} was refused for {} within a day: the team keeps trying it",
                    named.join(", ")
                ),
                newest,
            )?;
        }
        Ok(())
    }

    // ---- woken for nothing --------------------------------------------------------------------

    /// A wake refused because its member was woken on the task for nothing again and again
    /// (ADR 0124): a finding, and the thread says the work is parked for its lead.
    pub(super) fn found_redundant(&self, member: MemberId, task: TaskId) -> Done<()> {
        let current = self.task(task)?;
        let name = self.member(member)?.name;
        let evidence = format!(
            "{name} on #{} {}: {} turns in a row showed nothing",
            task.0, current.title, THRESHOLDS.redundant_wakes
        );
        // It rests on how many turns the member has had on the task: a new run of nothing after
        // a judged one has had turns since (ADR 0132).
        let had = self
            .store
            .runs()?
            .iter()
            .filter(|r| r.member == member && r.task == Some(task))
            .count();
        let found = self.find(
            Guard::RedundantWake,
            &format!("redundant:{}:{}", task.0, member.0),
            Some(task),
            current.project.map(|p| p.0),
            &evidence,
            i64::try_from(had).unwrap_or(i64::MAX),
        )?;
        if !found.how.is_news() {
            return Ok(());
        }
        self.post_held_lock(
            Author::Endora,
            task,
            &format!(
                "{name} was woken on this {} times in a row and nothing came of it: HQ stops \
                 waking {name} on it for nothing. It is parked: its lead decides it.",
                THRESHOLDS.redundant_wakes
            ),
        )
    }

    // ---- verdicts -----------------------------------------------------------------------------

    /// A lead gives a finding in its area its verdict (ADR 0124), in its turn.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`EngineError::Unknown`] for a finding that does not exist;
    /// [`Refusal::NotYours`] for a finding outside the lead's area.
    pub fn judge_finding_in(
        &self,
        run: RunId,
        id: i64,
        verdict: Verdict,
        fix_worked: Option<bool>,
    ) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let finding = self
            .store
            .findings()?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or(EngineError::Unknown("finding"))?;
        let lead = self.member(turn.member)?;
        let by_task = match finding.task {
            Some(task) => match self.store.task(TaskId(task))? {
                Some(task) => self.area_lead(&task)?.is_some_and(|l| l.id == lead.id),
                None => false,
            },
            None => false,
        };
        let by_project = match finding.project {
            Some(p) => self.project(crate::domain::ProjectId(p))?.lead == lead.id,
            None => false,
        };
        if !(by_task || by_project) {
            return Err(EngineError::Refused(Refusal::NotYours));
        }
        self.judge_held_lock(id, verdict, fix_worked, &lead.name)
    }

    /// The owner's verdict on a guard's notice from the desk: the finding is judged, and the
    /// notice put away.
    pub(super) fn desk_verdict(&self, item: crate::domain::DeskId, decision: &str) -> Done<()> {
        if decision == "dismiss" {
            return Ok(());
        }
        let verdict = Verdict::from_name(decision).ok_or(EngineError::Unknown("verdict"))?;
        let Some(finding) = self
            .store
            .findings()?
            .into_iter()
            .find(|f| f.desk == Some(item.0))
        else {
            return Err(EngineError::Unknown("finding"));
        };
        self.judge_held_lock(finding.id, verdict, None, "owner")
    }

    /// The guards as the screen shows them (ADR 0124): every threshold, each guard's
    /// precision, each project's ceremony as last looked, and the newest findings.
    pub(super) fn guards_shown(&self) -> Done<Value> {
        let findings = self.store.findings()?;
        let t = THRESHOLDS;
        let precision: Vec<Value> = guards::precision(&findings)
            .iter()
            .map(|p| {
                json!({ "guard": p.guard.name(), "plainly": p.guard.plainly(),
                        "rule": p.guard.rule(), "fired": p.fired, "open": p.open,
                        "real": p.real, "resolved": p.resolved,
                        "false_positive": p.false_positive, "not_enough": p.not_enough,
                        "fixed": p.fixed, "fix_judged": p.fix_judged,
                        "precision": p.percent() })
            })
            .collect();
        let ceremony: Value = self
            .store
            .setting(CEREMONY_SEEN)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!([]));
        let shown: Vec<Value> = findings
            .iter()
            .rev()
            .take(FINDINGS_SHOWN)
            .map(|f| {
                json!({ "id": f.id, "guard": f.guard.name(), "plainly": f.guard.plainly(),
                        "task": f.task, "project": f.project, "rule": f.rule,
                        "evidence": f.evidence, "opened_at": f.opened_at,
                        "seen_at": f.seen_at, "episodes": f.episodes,
                        "cleared_at": f.cleared_at, "verdict": f.verdict.map(Verdict::name),
                        "fix_worked": f.fix_worked, "judged_by": f.judged_by })
            })
            .collect();
        Ok(json!({
            "thresholds": {
                "ceremony_ratio": t.ceremony_ratio, "ceremony_least": t.ceremony_least,
                "carousel_returns": t.carousel_returns, "redundant_wakes": t.redundant_wakes,
                "fence_refusals_in_a_turn": t.fence_refusals_in_a_turn,
                "fence_members": t.fence_members, "fence_window_ms": t.fence_window_ms,
                "open_subtasks_most": t.open_subtasks_most, "depth_most": t.depth_most,
                "sufficient_turns": t.sufficient_turns,
                "retracted_percent": t.retracted_percent,
                "retracted_shared_least": t.retracted_shared_least,
            },
            "precision": precision,
            "ceremony": ceremony,
            "findings": shown,
        }))
    }
}
