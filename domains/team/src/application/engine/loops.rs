//! One team of loops (ADR 0142): every active project that is not a scenario world is a loop,
//! working its plan (its tasks, by priority then age), up to its items at once (two unless the
//! owner says) in hand together. A loop with room gives its top item to the member at its
//! starting rank, or the nearest one above it who is free to take work and holds none of its
//! items. Every member at work takes part: a member is its model.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{
    Brain, Member, MemberId, Millis, ProjectId, ProjectStatus, Refusal, Task, TaskId, TaskStatus,
    WakeReason,
};

/// The setting that keeps, by item, the reviewers passed over on its review (D26).
const PASSED_OVER: &str = "review_passed_over:";

/// The five members and their models, ranked (ADR 0142).
const SEATS: [(&str, Brain); 5] = [
    ("Mara", Brain::Opus),
    ("Kit", Brain::Sonnet),
    ("Ada", Brain::Haiku),
    ("Rae", Brain::Mac),
    ("Bo", Brain::Local),
];

/// Where the map's lines are kept: `[{line, by, at, last, uses}]`.
const MAP_KEY: &str = "map_lines";
/// How long a line nobody confirms is kept.
const MAP_FADES_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// The most characters of one line, and of the map in a briefing.
const MAP_LINE_MOST: usize = 190;
const MAP_SAID_MOST: usize = 1_400;
/// The most lines kept; past it, the least confirmed and oldest go.
const MAP_LINES_MOST: usize = 80;
/// How many of a loop's latest lines a turn on it is told.
const NOTES_MOST: usize = 6;

/// The one routine the loops keep: the lab's health check, as Bo's recurring item.
const HEALTH_CHECK: &str = "Check the home lab";

/// How many failed checks at one rank move an item up (ADR 0142).
const FAILURES_A_RANK: u32 = 3;

/// How many failures block an item of a loop worked alone (amended 2026-11-01): three ranks'
/// worth, the team arm's ceiling, so neither arm's spend on one item is unbounded.
const FAILURES_ALONE: u32 = 3 * FAILURES_A_RANK;

/// Where an item's failures at its rank are counted.
fn failures_key(task: TaskId) -> String {
    format!("loop_failures:{}", task.0)
}

/// Where an item held for an away member above is marked (review of 31 Oct): empty when not.
fn held_key(task: TaskId) -> String {
    format!("loop_held:{}", task.0)
}

/// The rank a loop starts at when the owner has not said: the house's model.
const START: u8 = Brain::BOTTOM;

/// Where a loop's starting rank is kept.
fn rank_key(project: ProjectId) -> String {
    format!("loop_rank:{}", project.0)
}

/// How many items a loop has in hand at once when the owner has not said (ADR 0142, amended
/// 2026-10-30), and the most the owner may set.
const ITEMS_AT_ONCE: u8 = 2;
const ITEMS_AT_ONCE_MOST: u8 = 4;

/// Where a loop's items at once are kept.
fn items_key(project: ProjectId) -> String {
    format!("loop_items:{}", project.0)
}

/// Where the member a loop is worked by alone is kept (ADR 0142, amended 2026-11-01): empty
/// when the team works it.
fn solo_key(project: ProjectId) -> String {
    format!("loop_solo:{}", project.0)
}

/// Whether an item is in hand: given to a member, and not yet in review or finished.
fn in_hand(task: &Task) -> bool {
    (task.assignee.is_some() || task.owner.is_some())
        && matches!(task.status, TaskStatus::Todo | TaskStatus::Doing)
}

/// Whether a member takes part in the loops: it is free to take work.
fn in_the_loops(member: &Member) -> bool {
    member.at_work()
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Whether nothing (a turn queued, started or ended, or a wake) has happened on `task` for
    /// longer than `ms`.
    pub(super) fn idle_for(&self, task: TaskId, ms: Millis) -> Done<bool> {
        let wakes = self.store.wakes()?;
        let last = self
            .store
            .runs_of(task)?
            .iter()
            .flat_map(|r| [Some(r.queued_at), r.started_at, r.ended_at])
            .flatten()
            .chain(wakes.iter().filter(|w| w.task == Some(task)).map(|w| w.at))
            .max()
            .unwrap_or(Millis::MIN);
        Ok(self.clock.now().saturating_sub(last) > ms)
    }

    /// The cut-over to one team of loops (ADR 0142), once, by the owner. Mara, Kit, Ada, Rae
    /// and Bo become the ranked models, plain members with no reporting line; every other
    /// member retires, its open work back on its loop's plan (one playing in a scenario
    /// retires when its game ends: run this again then). Only the lab's health check runs on,
    /// as Bo's. Running it again changes nothing that is done. Returns what it did.
    pub fn become_one_team(&self) -> Done<serde_json::Value> {
        let _held = self.held();
        let now = self.clock.now();
        let members = self.store.members()?;
        let tasks = self.store.tasks()?;
        let (mut seated, mut retired, mut playing) = (Vec::new(), Vec::new(), Vec::new());
        for member in &members {
            if let Some((_, model)) = SEATS
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&member.name))
            {
                self.store.set_brain(member.id, *model)?;
                seated.push(member.name.clone());
                continue;
            }
            if member.furloughed() {
                continue;
            }
            let held: Vec<&Task> = tasks
                .iter()
                .filter(|t| {
                    !t.status.finished()
                        && (t.owner == Some(member.id) || t.assignee == Some(member.id))
                })
                .collect();
            if self.any_in_a_scenario(&held)? {
                playing.push(member.name.clone());
                continue;
            }
            for task in held {
                self.let_go(task)?;
            }
            self.store.furlough_member(member.id, now)?;
            retired.push(member.name.clone());
        }
        let bo = self
            .store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case("Bo"));
        for mut routine in self.store.routines()? {
            let health = routine.title == HEALTH_CHECK;
            if let (true, Some(bo)) = (health, &bo) {
                routine.assignee = bo.id;
            }
            routine.enabled = health;
            self.store.save_routine(&routine)?;
        }
        Ok(serde_json::json!({ "seated": seated, "retired": retired, "still_playing": playing }))
    }

    /// The owner sets the rank a loop's items start at: 1 (Opus) to 6 (the house's model).
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project that does not exist; [`Refusal::Scenario`] for a
    /// rank outside 1 to 5; a store failure.
    pub fn set_loop_rank(&self, project: ProjectId, rank: u8) -> Done<()> {
        let _held = self.held();
        self.project(project)?;
        if !(1..=Brain::BOTTOM).contains(&rank) {
            return Err(EngineError::Refused(Refusal::Scenario(
                "a loop starts at a rank from 1 (Opus) to 6 (the house's model)".to_owned(),
            )));
        }
        self.store
            .set_setting(&rank_key(project), &rank.to_string())?;
        Ok(())
    }

    /// The rank a loop's items start at.
    pub(super) fn loop_rank(&self, project: ProjectId) -> Done<u8> {
        Ok(self
            .store
            .setting(&rank_key(project))?
            .and_then(|r| r.parse().ok())
            .unwrap_or(START))
    }

    /// The owner sets how many items a loop has in hand at once: 1 (one after another, as
    /// before) to 4. Each goes to a different member.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project that does not exist; [`Refusal::Scenario`] for a
    /// number outside 1 to 4; a store failure.
    pub fn set_loop_items(&self, project: ProjectId, items: u8) -> Done<()> {
        let _held = self.held();
        self.project(project)?;
        if !(1..=ITEMS_AT_ONCE_MOST).contains(&items) {
            return Err(EngineError::Refused(Refusal::Scenario(format!(
                "a loop has from 1 to {ITEMS_AT_ONCE_MOST} items in hand at once"
            ))));
        }
        self.store
            .set_setting(&items_key(project), &items.to_string())?;
        Ok(())
    }

    /// How many items a loop has in hand at once.
    pub fn loop_items(&self, project: ProjectId) -> Done<u8> {
        Ok(self
            .store
            .setting(&items_key(project))?
            .and_then(|n| n.parse().ok())
            .filter(|n| (1..=ITEMS_AT_ONCE_MOST).contains(n))
            .unwrap_or(ITEMS_AT_ONCE))
    }

    /// The owner sets a loop to be worked by one member alone, or (`None`) by the team again
    /// (ADR 0142, amended 2026-11-01: round 4's "one member, one long turn, no ladder" arm).
    /// Its items go to that member only, one at a time; nobody reviews them, a failure never
    /// climbs or hands one on, and the member takes no other loop's work nor reviews it.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a project or member that does not exist;
    /// [`Refusal::Scenario`] for a member no longer at work; a store failure.
    pub fn set_loop_solo(&self, project: ProjectId, member: Option<MemberId>) -> Done<()> {
        let _held = self.held();
        self.project(project)?;
        if let Some(member) = member {
            if self.member(member)?.furloughed() {
                return Err(EngineError::Refused(Refusal::Scenario(
                    "a loop is worked alone by a member at work".to_owned(),
                )));
            }
        }
        let kept = member.map(|m| m.0.to_string()).unwrap_or_default();
        self.store.set_setting(&solo_key(project), &kept)?;
        Ok(())
    }

    /// The member who works a loop alone, if the owner named one.
    pub fn loop_solo(&self, project: ProjectId) -> Done<Option<MemberId>> {
        Ok(self
            .store
            .setting(&solo_key(project))?
            .and_then(|v| v.parse().ok())
            .map(MemberId))
    }

    /// The member who works an item's loop alone, if its loop is so worked.
    pub(super) fn solo_of(&self, task: &Task) -> Done<Option<MemberId>> {
        let Some(project) = task.project else {
            return Ok(None);
        };
        self.loop_solo(project)
    }

    /// What a turn on an item of a loop its member works alone is told of that (amended
    /// 2026-11-01): nobody reviews it or takes it over, and setting it ready closes it once HQ's
    /// check has passed. Empty for any other turn.
    pub(super) fn alone_said(
        &self,
        member: &Member,
        run: &crate::application::ports::Run,
    ) -> Done<String> {
        let Some(task) = run.task else {
            return Ok(String::new());
        };
        let item = self.task(task)?;
        if self.solo_of(&item)? != Some(member.id) {
            return Ok(String::new());
        }
        // Climb 3, round 7: the way to done is said as it is for this item. Only work that needs
        // the gate waits for HQ's check of its newest head (ready_alone); a routine's item or the
        // home lab's needs none, and a repository outside the lab has no check of the lab's at all
        // (calibration ST rep 2 waited for one past the episode's end).
        let gate = crate::domain::review::needs_gate(
            self.routine_of(item.id)?.is_some(),
            self.the_home_labs(&item)?,
        );
        let outside = self.outside_repo_of_task(item.id)?.is_some();
        Ok(format!(
            "You work this loop alone: nobody reviews your item and nobody takes it over. Check \
             its criteria yourself. {} Three failures do not take it a rank up: after \
             {FAILURES_ALONE} it is blocked; if it cannot be done, block it yourself with the \
             reason.\n",
            alone_route(gate, outside)
        ))
    }

    /// The members who work a loop alone (one active or paused): out of the team's loops.
    fn solo_members(&self) -> Done<Vec<MemberId>> {
        let mut solo = Vec::new();
        for project in self.store.projects()? {
            if !matches!(
                project.status,
                ProjectStatus::Active | ProjectStatus::Paused
            ) {
                continue;
            }
            solo.extend(self.loop_solo(project.id)?);
        }
        Ok(solo)
    }

    /// The members who take part in the team's loops: those in the loops, less any who work a
    /// loop alone. Climbs, hand-ups, reviews and the team's hand-outs draw on these.
    fn team_members(&self) -> Done<Vec<Member>> {
        let solo = self.solo_members()?;
        let mut members = self.loop_members()?;
        members.retain(|m| !solo.contains(&m.id));
        Ok(members)
    }

    /// The members who take part in the loops.
    pub(super) fn loop_members(&self) -> Done<Vec<Member>> {
        Ok(self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect())
    }

    /// The member who holds an item, if it is one who takes part in the loops.
    fn loop_holder(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = task.holder() else {
            return Ok(None);
        };
        Ok(self.loop_members()?.into_iter().find(|m| m.id == holder))
    }

    /// The member nearest above `rank` (a smaller number), if any.
    fn one_rank_up(members: &[Member], rank: u8) -> Option<&Member> {
        members
            .iter()
            .filter(|m| m.brain.rank() < rank)
            .max_by_key(|m| (m.brain.rank(), std::cmp::Reverse(m.id.0)))
    }

    /// Who reviews an item in the loops (ADR 0142): the member one rank above its holder, and
    /// the top rank's work the member just below it; never above Haiku's rank for work done
    /// below it (amended 2026-10-31, D7: [`crate::domain::review::reviewer_among`]). `None` for
    /// work outside the loops. Nobody reviews work she had a hand in ([`Self::had_a_hand_in`]),
    /// nor this review again once her turn on it ended with no verdict ([`Self::passed_over`]).
    pub(crate) fn loop_reviewer_of(&self, task: &Task) -> Done<Option<Member>> {
        let Some(holder) = self.loop_holder(task)? else {
            return Ok(None);
        };
        // A loop worked alone has no reviewer (amended 2026-11-01).
        if self.solo_of(task)?.is_some() {
            return Ok(None);
        }
        let mut not_them = self.had_a_hand_in(task.id)?;
        not_them.extend(self.passed_over(task.id)?);
        // Nor does a member who works a loop alone review the team's work.
        not_them.extend(self.solo_members()?);
        let members: Vec<Member> = self
            .loop_members()?
            .into_iter()
            .filter(|m| m.id == holder.id || !not_them.contains(&m.id))
            .collect();
        let ranked: Vec<_> = members.iter().map(|m| (m.id, m.brain.rank())).collect();
        let reviewer =
            crate::domain::review::reviewer_among(holder.id, holder.brain.rank(), &ranked);
        Ok(reviewer.and_then(|id| members.into_iter().find(|m| m.id == id)))
    }

    /// The reviewers passed over on an item's review as it stands: their review turn ended with
    /// neither verdict (D26). Cleared when the item is asked for review again.
    pub(super) fn passed_over(&self, task: TaskId) -> Done<Vec<MemberId>> {
        Ok(self
            .store
            .setting(&format!("{PASSED_OVER}{}", task.0))?
            .unwrap_or_default()
            .split(',')
            .filter_map(|id| id.parse().ok().map(MemberId))
            .collect())
    }

    /// Clears the reviewers passed over on an item: its review starts again.
    pub(super) fn review_starts_again(&self, task: TaskId) -> Done<()> {
        self.store
            .set_setting(&format!("{PASSED_OVER}{}", task.0), "")?;
        Ok(())
    }

    /// After a turn that ended (the lock held), D26 of round 3: Rae reviewed three items with
    /// one checkout each and ended with no verdict and no post, and each sat in review. A review
    /// turn that ends with neither verdict, the item still in review and still hers to review,
    /// and not out of room (which goes on afresh), is recorded as not done: she is passed over,
    /// and the next reviewer by the same rule is asked, or with nobody left, the owner.
    pub(super) fn after_review_turn(&self, run: &crate::application::ports::Run) -> Done<()> {
        let Some(task) = run.task else {
            return Ok(());
        };
        if self.reason_for(run)? != WakeReason::ReviewRequested.name()
            || self.ran_out_of_room(run)?
        {
            return Ok(());
        }
        let found = self.task(task)?;
        if found.status != TaskStatus::InReview
            || self
                .loop_reviewer_of(&found)?
                .is_none_or(|r| r.id != run.member)
        {
            return Ok(());
        }
        let mut over = self.passed_over(task)?;
        over.push(run.member);
        let kept: Vec<String> = over.iter().map(|m| m.0.to_string()).collect();
        self.store
            .set_setting(&format!("{PASSED_OVER}{}", task.0), &kept.join(","))?;
        let name = self.member(run.member)?.name;
        let said = format!("{name}'s review ended with neither verdict: not done.");
        let Some(next) = self.loop_reviewer_of(&found)? else {
            self.post_held_lock(super::Author::Endora, task, &said)?;
            return self.ask_about_the_item(
                task,
                None,
                &format!(
                    "Nobody is left to review #{}: each review turn on it ended with neither \
                     verdict. Read it yourself, or move it back to work.",
                    task.0
                ),
            );
        };
        self.post_held_lock(
            super::Author::Endora,
            task,
            &format!("{said} {} reviews this.", next.name),
        )?;
        self.wake_held_lock(next.id, Some(task), WakeReason::ReviewRequested)?;
        Ok(())
    }

    /// The members who had a hand in an item's work (D24 of round 3: Ada reviewed and closed
    /// 952, whose branch carried her commits, because HQ asked only who submitted its head):
    /// whoever kept a branch of it, its sub-tasks' included, and whoever took a turn of work on
    /// it (taking it, going on with it, or woken back to it), as against a review.
    pub(super) fn had_a_hand_in(&self, task: TaskId) -> Done<Vec<MemberId>> {
        let work = self.work_of(task)?;
        let mut hands: Vec<MemberId> = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| work.contains(&b.task))
            .map(|b| b.member)
            .collect();
        for run in self.store.runs_of(task)? {
            if run.started_at.is_none() || hands.contains(&run.member) {
                continue;
            }
            let worked = self.store.wake(run.wake)?.is_some_and(|w| {
                matches!(
                    w.reason,
                    WakeReason::Assigned
                        | WakeReason::AssignmentRecovery
                        | WakeReason::Continued
                        | WakeReason::Recovery
                        | WakeReason::SentBack
                        | WakeReason::CheckDone
                )
            });
            if worked {
                hands.push(run.member);
            }
        }
        Ok(hands)
    }

    /// Whether an item is held by a member in the loops: its stalls and failures climb the
    /// ranks, not the ladder, and no guard of coordination looks at it.
    pub(crate) fn loop_holds(&self, task: &Task) -> Done<bool> {
        Ok(self.loop_holder(task)?.is_some())
    }

    /// Whether a member takes part in the loops.
    pub(crate) fn in_the_loops(&self, member: MemberId) -> Done<bool> {
        Ok(self.loop_members()?.iter().any(|m| m.id == member))
    }

    /// An item that sat with nothing moving it, after its one nudge: one of its failures. Kept
    /// at its rank, its holder is woken to try again; past the third, it has climbed.
    ///
    /// Work in review waits on its reviewer, not its holder (D17 of round 3: task 947 counted
    /// "Failure 1 of 3 at Mara's rank" while it waited, and was blocked as failed at every
    /// rank): no failure, no climb, and the holder, whose wake HQ would refuse, is not woken.
    /// The reviewer is asked again, once for each time the item has moved since.
    pub(super) fn loop_stalled(&self, task: &Task) -> Done<()> {
        if task.status == TaskStatus::InReview {
            if let Some(reviewer) = self.loop_reviewer_of(task)? {
                self.nudge_once(task, reviewer.id, WakeReason::ReviewRequested)?;
            }
            return Ok(());
        }
        let holder = task.holder();
        self.loop_failed(task.id)?;
        let now = self.task(task.id)?;
        if now.holder() == holder && !self.held_above(task.id)? {
            if let Some(holder) = holder {
                self.wake_held_lock(holder, Some(task.id), WakeReason::Recovery)?;
            }
        }
        Ok(())
    }

    /// Whether an item is blocked in the loops (ADR 0142, amended 2026-10-31): blocked, on a
    /// loop's plan while the loops run, and not a scenario's. Such an item waits for the owner,
    /// and only their word, or their move of it back to work, wakes anyone on it.
    pub(super) fn blocked_in_the_loops(&self, task: TaskId) -> Done<bool> {
        let found = self.task(task)?;
        if found.status != TaskStatus::Blocked || found.project.is_none() {
            return Ok(false);
        }
        if self.loop_members()?.is_empty() || self.in_a_scenario(task)? {
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether a wake for `reason` on `task` waits, because the item is blocked in the loops and
    /// waits for the owner (ADR 0142, amended 2026-10-31): only a reason that reaches a blocked
    /// item (the owner's word, the end of what it waits on) wakes anyone on it, and the owner's
    /// decision only with words (`said`, when the wake carries the owner's). The one check of
    /// a blocked item for wakes, nudges and the owner's answers alike.
    pub(super) fn waits_for_the_owner(
        &self,
        task: TaskId,
        reason: WakeReason,
        said: Option<&str>,
    ) -> Done<bool> {
        let reaches = reason.reaches_a_blocked_item() && said.is_none_or(|w| !w.trim().is_empty());
        if reaches {
            return Ok(false);
        }
        self.blocked_in_the_loops(task)
    }

    /// HQ asks the owner about an item: one question waits on the desk per item (ADR 0142,
    /// amended 2026-10-31). With one already waiting, `text` is added to that card; otherwise it
    /// is a new card, from `member` when a member's act raised it.
    pub(super) fn ask_about_the_item(
        &self,
        task: TaskId,
        member: Option<MemberId>,
        text: &str,
    ) -> Done<()> {
        self.put_on_the_desk("question", task, member, text, false)?;
        Ok(())
    }

    /// A member blocked its item in the loops: since nothing but the owner now moves it, the
    /// owner is asked, once (ADR 0142, amended 2026-10-31).
    pub(super) fn loop_blocked(&self, task: TaskId, member: MemberId, reason: &str) -> Done<()> {
        if !self.blocked_in_the_loops(task)? {
            return Ok(());
        }
        self.keep_blocked_submission(task)?;
        let found = self.task(task)?;
        let who = self.member(member)?;
        let text = format!(
            "{} blocked #{} ({}): {}. Nobody is woken on it until you answer here in words, or \
             move it back to work.",
            who.name,
            task.0,
            found.title,
            super::one_line(reason, 600)
        );
        self.ask_about_the_item(task, Some(member), &text)
    }

    /// A notice made while the loops run is kept and told, but never waits on the owner's desk
    /// (ADR 0142): the desk is for what only the owner decides.
    pub(super) fn quiet_in_the_loops(&self, desk: crate::domain::DeskId) -> Done<()> {
        if !self.loop_members()?.is_empty() {
            self.store.set_desk_status(desk, "seen")?;
        }
        Ok(())
    }

    /// The map's lines still kept at `now`, the most confirmed first, then the latest.
    fn map_lines(&self, now: crate::domain::Millis) -> Done<Vec<serde_json::Value>> {
        let mut lines: Vec<serde_json::Value> = self.setting_json(MAP_KEY)?.unwrap_or_default();
        lines.retain(|l| now - l["last"].as_i64().unwrap_or(0) < MAP_FADES_MS);
        lines.sort_by_key(|l| {
            (
                std::cmp::Reverse(l["uses"].as_i64().unwrap_or(0)),
                std::cmp::Reverse(l["last"].as_i64().unwrap_or(0)),
            )
        });
        Ok(lines)
    }

    /// A member adds a line to the map (ADR 0142): where something lives, found in its turn.
    /// The same line again confirms it. Returns what the member is told.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`] for a turn that is not running; [`EngineError::Unknown`] for an
    /// empty line; a store failure.
    pub fn add_to_map(&self, run: crate::domain::RunId, line: &str) -> Done<String> {
        let _held = self.held();
        let run = self.running(run)?;
        let member = self.member(run.member)?;
        let line = super::one_line(line, MAP_LINE_MOST);
        if line.is_empty() {
            return Err(EngineError::Unknown("map line"));
        }
        let now = self.clock.now();
        let mut lines = self.map_lines(now)?;
        let same = |l: &serde_json::Value| {
            l["line"]
                .as_str()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case(line.trim()))
        };
        let said = if let Some(found) = lines.iter_mut().find(|l| same(l)) {
            found["uses"] = serde_json::json!(found["uses"].as_i64().unwrap_or(0) + 1);
            found["last"] = serde_json::json!(now);
            "It was on the map already: you confirmed it."
        } else {
            lines.push(
                serde_json::json!({ "line": line, "by": member.name, "at": now,
                                           "last": now, "uses": 1 }),
            );
            "It is on the map: every member's briefing shows it."
        };
        lines.truncate(MAP_LINES_MOST);
        self.store
            .set_setting(MAP_KEY, &serde_json::Value::Array(lines).to_string())?;
        Ok(said.to_owned())
    }

    /// The map's lines, whole, most confirmed first, as `read_world world:map` gives them. They
    /// are members' words: the turn's input is marked as of unknown origin.
    pub(super) fn map_lines_read(&self, run: crate::domain::RunId) -> Done<String> {
        let lines = self.map_lines(self.clock.now())?;
        if lines.is_empty() {
            return Ok("The map (world:map) has no lines yet: add one with add_to_map.".to_owned());
        }
        self.unclassified_message_input(run)?;
        let mut said =
            String::from("The map, where things are (world:map), most confirmed first:\n");
        for l in &lines {
            said.push_str(&format!("- {}\n", l["line"].as_str().unwrap_or("")));
        }
        Ok(said)
    }

    /// The map's lines for the owner: each with who found it, when, and how often confirmed.
    pub(super) fn map_lines_shown(&self) -> Done<serde_json::Value> {
        let lines = self.map_lines(self.clock.now())?;
        Ok(serde_json::json!(
            lines
                .iter()
                .map(
                    |l| serde_json::json!({ "line": l["line"], "by": l["by"], "at": l["at"],
                                              "uses": l["uses"] })
                )
                .collect::<Vec<_>>()
        ))
    }

    /// Whether any of these tasks is a scenario's.
    fn any_in_a_scenario(&self, tasks: &[&Task]) -> Done<bool> {
        for task in tasks {
            if self.in_a_scenario(task.id)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether `member` thinks with the Mac and the Mac is away now: the node said it is not
    /// free, or has not said lately. Before the node has said anything, it is not away.
    pub(super) fn mac_away_for(&self, member: &Member) -> bool {
        if member.brain != crate::domain::Brain::Mac {
            return false;
        }
        let Some(facts) = self.mac_facts() else {
            return false;
        };
        crate::domain::mac::wait_for(Some(&facts), self.clock.now(), false).is_some()
    }

    /// The items a paused loop member holds go to the member one rank up (owner, 30 Oct: the
    /// Mac's member is paused while the owner uses the laptop), as a climb does but counting
    /// no failure. An item with nobody above stays where it is.
    fn paused_let_go(&self, at_work: &[Member]) -> Done<()> {
        let paused: Vec<Member> = self
            .store
            .members()?
            .into_iter()
            .filter(|m| !m.furloughed() && (m.paused || self.mac_away_for(m)))
            .collect();
        if paused.is_empty() {
            return Ok(());
        }
        for task in self.store.tasks()? {
            if task.project.is_none()
                || !matches!(task.status, TaskStatus::Todo | TaskStatus::Doing)
            {
                continue;
            }
            let Some(holder) = task
                .owner
                .or(task.assignee)
                .and_then(|id| paused.iter().find(|m| m.id == id))
            else {
                continue;
            };
            // A scenario's item, or one of a loop worked alone, waits for its holder.
            if self.in_a_scenario(task.id)? || self.solo_of(&task)?.is_some() {
                continue;
            }
            // A turn of the holder's still running on it ends first; the next tick hands it on.
            let mid_turn = self.store.live_runs()?.iter().any(|r| {
                r.member == holder.id
                    && r.task == Some(task.id)
                    && r.status == crate::domain::RunStatus::Running
            });
            if mid_turn {
                continue;
            }
            let Some(up) = Self::one_rank_up(at_work, holder.brain.rank()).or_else(|| {
                at_work
                    .iter()
                    .filter(|m| m.brain.rank() > holder.brain.rank())
                    .min_by_key(|m| (m.brain.rank(), m.id.0))
            }) else {
                continue;
            };
            let up = up.clone();
            self.store.release(task.id)?;
            self.store.set_assignee(task.id, Some(up.id))?;
            if task.status != TaskStatus::Todo {
                self.store.set_task_status(task.id, TaskStatus::Todo)?;
            }
            let why = format!(
                "{} is {}",
                holder.name,
                if holder.paused {
                    "paused"
                } else {
                    "away (the Mac is not free)"
                }
            );
            self.post_held_lock(
                super::Author::Endora,
                task.id,
                &format!("{why}: it goes to {}.", up.name),
            )?;
            self.hand_over(task.id, up.id, &why)?;
            self.wake_held_lock(up.id, Some(task.id), WakeReason::Assigned)?;
            // The holder's turns still queued on it (waiting for the Mac, or a retry) end: the
            // item is the next member's now (review of 30 Oct).
            for run in self.store.live_runs()? {
                if run.member == holder.id
                    && run.task == Some(task.id)
                    && run.status != crate::domain::RunStatus::Running
                {
                    self.end(
                        run,
                        crate::domain::RunStatus::Stopped,
                        "its item went one rank up",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// A retired member's task is let go: unclaimed, unassigned, and back to todo if begun.
    fn let_go(&self, task: &Task) -> Done<()> {
        self.store.release(task.id)?;
        self.store.set_assignee(task.id, None)?;
        if task.status == TaskStatus::Doing {
            self.store.set_task_status(task.id, TaskStatus::Todo)?;
        }
        Ok(())
    }

    /// What a turn in the loops is told besides its task (ADR 0142): the map, most confirmed
    /// first, and what its loop did lately: its members' notes and the owner's. Members' words
    /// are among them, so the turn's input is marked as of unknown origin.
    pub(super) fn loops_said(&self, run: &crate::application::ports::Run) -> Done<String> {
        let mut told = String::new();
        let relevance = self.item_relevance(run)?;
        // On an item, the map's lines about it, and where the rest is (amended 2026-10-30).
        let lines = self.map_lines(self.clock.now())?;
        let (lines, left) = match &relevance {
            Some(r) => r.keep(lines, |l| l["line"].as_str().unwrap_or("").to_owned()),
            None => (lines, 0),
        };
        if !lines.is_empty() {
            self.unclassified_message_input(run.id)?;
            told.push_str(
                "The map, where things are (add_to_map when you find one; the same line again \
                 confirms it):\n",
            );
            map_lines_within(&mut told, &lines);
        }
        if left > 0 {
            told.push_str(&format!(
                "{left} more lines on the map are about other things: read_world world:map.\n"
            ));
        }
        let Some(task) = run.task else {
            return Ok(told);
        };
        let Some(project) = self.task(task)?.project else {
            return Ok(told);
        };
        // Its members' words, and the owner's (climb 2, 6 Nov): the owner's decision on one of the
        // loop's items reaches its next item, or the member asks the same question again.
        let mut names: Vec<String> = self.store.members()?.into_iter().map(|m| m.name).collect();
        names.push(self.author_name(super::Author::Owner)?);
        let mut notes: Vec<(TaskId, String, crate::application::ports::Post)> = Vec::new();
        for other in self.store.tasks()? {
            if other.project != Some(project) || other.id == task {
                continue;
            }
            let theirs = self.store.thread(other.id)?.into_iter();
            notes.extend(
                theirs
                    .filter(|p| names.contains(&p.author))
                    .map(|p| (other.id, other.title.clone(), p)),
            );
        }
        // On an item, the members' notes of the loop's items that name what it names; the
        // owner's always (climb 2, 6 Nov: a short answer, "Leave it as it is; do not change it.",
        // was dropped when the members had repeated its only word of the item's).
        let owner = self.author_name(super::Author::Owner)?;
        let (owners, members): (Vec<_>, Vec<_>) =
            notes.into_iter().partition(|(_, _, p)| p.author == owner);
        let (mut notes, skipped) = match &relevance {
            Some(r) => r.keep(members, |(_, title, p)| format!("{title} {}", p.text)),
            None => (members, 0),
        };
        notes.extend(owners);
        let mut lately: Vec<(crate::domain::Millis, String)> = notes
            .into_iter()
            .map(|(id, _, post)| {
                let said = super::one_line(&post.text, 200);
                (post.at, format!("- {} on #{}: {said}\n", post.author, id.0))
            })
            .collect();
        let pointer = format!(
            "The loop's notes on its other items: ask_record with project {}.\n",
            project.0
        );
        if lately.is_empty() {
            if skipped > 0 {
                told.push_str(&pointer);
            }
            return Ok(told);
        }
        self.unclassified_message_input(run.id)?;
        lately.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        told.push_str("What this loop did lately, newest first:\n");
        for (_, said) in lately.into_iter().take(NOTES_MOST) {
            told.push_str(&said);
        }
        if skipped > 0 {
            told.push_str(&pointer);
        }
        Ok(told)
    }

    /// An item's failures at its present rank.
    pub fn loop_failures(&self, task: TaskId) -> Done<u32> {
        Ok(self
            .store
            .setting(&failures_key(task))?
            .and_then(|n| n.parse().ok())
            .unwrap_or(0))
    }

    /// One of an item's failures (a failed check, or a send-back), the lock held. The third at
    /// a rank hands it to the member one rank up; the top rank's third goes to the owner.
    pub(crate) fn loop_failed(&self, task: TaskId) -> Done<()> {
        let found = self.task(task)?;
        let Some(holder) = self.loop_holder(&found)? else {
            return Ok(());
        };
        if self.solo_of(&found)?.is_some() {
            return self.failed_alone(&found, &holder);
        }
        // Held for an away member above: it neither counts nor climbs until one is free.
        if self.held_above(task)? {
            return Ok(());
        }
        let failures = self.loop_failures(task)? + 1;
        if failures < FAILURES_A_RANK {
            self.store
                .set_setting(&failures_key(task), &failures.to_string())?;
            return self.post_held_lock(
                super::Author::Endora,
                task,
                &format!(
                    "Failure {failures} of {FAILURES_A_RANK} at {}'s rank.",
                    holder.name
                ),
            );
        }
        self.store.set_setting(&failures_key(task), "0")?;
        let all = self.team_members()?;
        let members: Vec<Member> = all
            .iter()
            .filter(|m| !self.mac_away_for(m))
            .cloned()
            .collect();
        let Some(up) = Self::one_rank_up(&members, holder.brain.rank()) else {
            // Only the away Mac's member is above (review of 31 Oct): a laptop in use for an
            // evening must not block the item for good. It waits where it is.
            if let Some(away) = Self::one_rank_up(&all, holder.brain.rank()) {
                return self.hold_for(&found, &holder, away);
            }
            // Blocked and let go, so its loop hands out its next item and nobody is woken to
            // fail it again (the baseline of 28 Oct: it stayed doing and stalled its loop).
            self.store.release(task)?;
            self.store.set_assignee(task, None)?;
            self.store.set_task_status(task, TaskStatus::Blocked)?;
            self.keep_blocked_submission(task)?;
            let text = format!(
                "{} failed at every rank, {} the last to try it. It is blocked and its loop has \
                 moved on; its thread says what was tried.",
                found.title, holder.name
            );
            return self.ask_about_the_item(task, None, &text);
        };
        self.climb(&found, &holder, up.id)
    }

    /// One of the failures of an item whose loop `holder` works alone (amended 2026-11-01):
    /// counted and said, and it stays with `holder`. Nothing climbs and nothing is handed on.
    /// The [`FAILURES_ALONE`]th blocks it with the reason and asks the owner, as the team's top
    /// rank's last failure does.
    fn failed_alone(&self, found: &Task, holder: &Member) -> Done<()> {
        let task = found.id;
        let failures = self.loop_failures(task)? + 1;
        self.store
            .set_setting(&failures_key(task), &failures.to_string())?;
        if failures < FAILURES_ALONE {
            return self.post_held_lock(
                super::Author::Endora,
                task,
                &format!(
                    "Failure {failures} of {FAILURES_ALONE}: {} works this loop alone, so it \
                     stays with {}.",
                    holder.name, holder.name
                ),
            );
        }
        self.store.release(task)?;
        self.store.set_assignee(task, None)?;
        self.store.set_task_status(task, TaskStatus::Blocked)?;
        self.keep_blocked_submission(task)?;
        let text = format!(
            "{} failed {FAILURES_ALONE} times with {} working this loop alone. It is blocked and \
             its loop has moved on; its thread says what was tried.",
            found.title, holder.name
        );
        self.post_held_lock(super::Author::Endora, task, &text)?;
        self.ask_about_the_item(task, None, &text)
    }

    /// The item goes one rank up, to `up`, after [`FAILURES_A_RANK`] failures at `holder`'s.
    fn climb(&self, found: &Task, holder: &Member, up: MemberId) -> Done<()> {
        let task = found.id;
        let to = self.member(up)?;
        self.store.release(task)?;
        self.store.set_assignee(task, Some(up))?;
        if found.status != TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Todo)?;
        }
        self.post_held_lock(
            super::Author::Endora,
            task,
            &format!(
                "It failed {FAILURES_A_RANK} times at {}'s rank: it goes to {}, one rank up.",
                holder.name, to.name
            ),
        )?;
        self.hand_over(
            task,
            up,
            &format!(
                "it failed {FAILURES_A_RANK} times at {}'s rank",
                holder.name
            ),
        )?;
        self.wake_held_lock(up, Some(task), WakeReason::Assigned)?;
        Ok(())
    }

    /// Whether an item is held for an away member above its holder.
    pub(super) fn held_above(&self, task: TaskId) -> Done<bool> {
        Ok(self
            .store
            .setting(&held_key(task))?
            .is_some_and(|v| !v.is_empty()))
    }

    /// The item failed at its holder's rank and only `away` (the Mac's member, the Mac not
    /// free) is above (review of 31 Oct): it stays with its holder, let go and back to todo,
    /// and nobody is woken on it until a member above is free ([`Self::climb_held`]).
    fn hold_for(&self, found: &Task, holder: &Member, away: &Member) -> Done<()> {
        let task = found.id;
        self.store
            .set_setting(&held_key(task), &holder.id.0.to_string())?;
        self.store.release(task)?;
        if found.status != TaskStatus::Todo {
            self.store.set_task_status(task, TaskStatus::Todo)?;
        }
        self.post_held_lock(
            super::Author::Endora,
            task,
            &format!(
                "It failed {FAILURES_A_RANK} times at {}'s rank, and {}, one rank up, is away \
                 (the Mac is not free): it waits for {} here, and nobody is woken on it until \
                 a member above is free.",
                holder.name, away.name, away.name
            ),
        )
    }

    /// Each held item whose holder still has it goes one rank up once a member above is free
    /// (`at_work`: the loop members not away); one no longer its holder's is let out of hold.
    fn climb_held(&self, at_work: &[Member]) -> Done<()> {
        for task in self.store.tasks()? {
            let Some(kept) = self.store.setting(&held_key(task.id))? else {
                continue;
            };
            let Ok(held_by) = kept.parse::<i64>() else {
                continue;
            };
            let holder = task.holder();
            let still = holder.is_some_and(|h| h.0 == held_by) && in_hand(&task);
            let Some(member) = at_work.iter().find(|m| still && m.id.0 == held_by) else {
                if !still {
                    self.store.set_setting(&held_key(task.id), "")?;
                }
                continue;
            };
            let Some(up) = Self::one_rank_up(at_work, member.brain.rank()) else {
                continue;
            };
            let up = up.id;
            self.store.set_setting(&held_key(task.id), "")?;
            self.climb(&task, member, up)?;
        }
        Ok(())
    }

    /// The loops' part of the tick: a paused member's items go one rank up, then each loop with
    /// fewer items in hand than it has at once hands out its top items, each to a different
    /// member at or nearest above its starting rank. A world's project is not a loop: its games
    /// go one at a time, as the league sets them.
    pub(super) fn loops_tick(&self) -> Done<()> {
        let members: Vec<Member> = self
            .store
            .members()?
            .into_iter()
            .filter(in_the_loops)
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        // The Mac's member is passed over while the Mac is away (the owner's laptop, 30 Oct).
        let members: Vec<Member> = members
            .into_iter()
            .filter(|m| !self.mac_away_for(m))
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        // A member who works a loop alone is in no hand-up, climb or hand-out of the team's.
        let solo = self.solo_members()?;
        let team: Vec<Member> = members
            .iter()
            .filter(|m| !solo.contains(&m.id))
            .cloned()
            .collect();
        self.paused_let_go(&team)?;
        self.climb_held(&team)?;
        let tasks = self.store.tasks()?;
        for project in self.store.projects()? {
            if project.status != ProjectStatus::Active || project.pause.is_some() {
                continue;
            }
            if self.holds_scenarios(project.id).is_ok() {
                continue;
            }
            let plan: Vec<_> = tasks
                .iter()
                .filter(|t| t.project == Some(project.id))
                .collect();
            let at_once = usize::from(self.loop_items(project.id)?);
            // Who holds the loop's items in hand now: the next goes to someone else.
            let mut holding: Vec<MemberId> = plan
                .iter()
                .filter(|t| in_hand(t))
                .filter_map(|t| t.holder())
                .collect();
            let held = plan.iter().filter(|t| in_hand(t)).count();
            let mut waiting: Vec<&&Task> = plan
                .iter()
                .filter(|t| {
                    t.status == TaskStatus::Todo && t.assignee.is_none() && t.owner.is_none()
                })
                .collect();
            waiting.sort_by_key(|t| (t.priority, t.id.0));
            let start = self.loop_rank(project.id)?;
            // The members at the starting rank or nearest above it, as many as the loop has
            // items at once: an item never climbs further for want of a free member.
            // A loop worked alone hands its items to its member only, one at a time.
            let mut near: Vec<&Member> = match self.loop_solo(project.id)? {
                Some(alone) => members.iter().filter(|m| m.id == alone).collect(),
                None => team.iter().filter(|m| m.brain.rank() <= start).collect(),
            };
            near.sort_by_key(|m| (std::cmp::Reverse(m.brain.rank()), m.id.0));
            near.truncate(at_once);
            // As many as the loop has room for: each pass hands out exactly one item, or stops.
            for top in waiting.into_iter().take(at_once.saturating_sub(held)) {
                let Some(member) = near.iter().find(|m| !holding.contains(&m.id)) else {
                    break;
                };
                self.store.set_assignee(top.id, Some(member.id))?;
                self.post_held_lock(
                    super::Author::Endora,
                    top.id,
                    &format!(
                        "{} takes this: the next item of the {} loop.",
                        member.name, project.title
                    ),
                )?;
                self.wake_held_lock(member.id, Some(top.id), WakeReason::Assigned)?;
                holding.push(member.id);
            }
        }
        Ok(())
    }
}

/// The map's lines, most confirmed first, as many as fit in [`MAP_SAID_MOST`].
fn map_lines_within(told: &mut String, lines: &[serde_json::Value]) {
    for l in lines {
        let said = format!("- {}\n", l["line"].as_str().unwrap_or(""));
        if told.len() + said.len() > MAP_SAID_MOST {
            return;
        }
        told.push_str(&said);
    }
}

/// How an item of a loop worked alone becomes done, as it is for that item (climb 3, round 7):
/// `gate` when the work needs the gate (HQ's check of its newest head) before it is set ready,
/// `outside` when its repository is outside the lab, where no check of the lab's applies.
#[must_use]
pub(super) fn alone_route(gate: bool, outside: bool) -> &'static str {
    if gate && !outside {
        return "Once HQ's check of your newest submission has passed, setting it ready \
                (set_status in_review) closes it, with no review.";
    }
    if outside {
        // Round 8: changes are submitted, but work that needed none closes without a branch
        return "This item's repository is outside the lab, so no check of the lab's applies to \
                it: there is nothing to wait for. When the work meets its criteria, submit any \
                changes with repo_submit before setting it ready (set_status in_review). If no \
                changes were needed, do not create a branch merely to close the item. Setting it \
                ready closes it, with no review. A submitted branch is kept on the node for \
                judging, never pushed or deployed.";
    }
    "This item needs no check of HQ's first: no gate stands between your work and setting it \
     ready. When it meets its criteria (and any branch is submitted), setting it ready \
     (set_status in_review) closes it, with no review."
}

#[cfg(test)]
mod alone_route_tests {
    use super::alone_route;

    #[test]
    fn only_gated_lab_work_waits_for_hqs_check() {
        assert!(alone_route(true, false).contains("Once HQ's check of your newest submission"));
        for (gate, outside) in [(false, false), (false, true), (true, true)] {
            let said = alone_route(gate, outside);
            assert!(!said.contains("Once HQ's check"), "{said}");
            assert!(said.contains("set_status in_review"), "{said}");
            assert!(said.contains("closes it, with no review"), "{said}");
        }
    }

    #[test]
    fn an_outside_item_is_told_there_is_nothing_to_wait_for_and_nothing_is_deployed() {
        let said = alone_route(false, true);
        assert!(said.contains("no check of the lab's applies to it: there is nothing to wait for"));
        assert!(said.contains("submit any changes with repo_submit before setting it ready"));
        assert!(said.contains(
            "If no changes were needed, do not create a branch merely to close the item"
        ));
        assert!(said.contains("never pushed or deployed"));
        assert!(!alone_route(false, false).contains("outside the lab"));
    }
}
