//! Projects (ADR 0113): a piece of one goal, with its own lead and, if the owner gives it one,
//! its own monthly cap. Goals stay at the top; a task belongs to a goal and, if it is part of
//! one, to a project serving that goal. A project's money is counted by the calendar month in
//! UTC, from the ledger's rows for its tasks' turns.
//!
//! Models propose; these rules decide: who may lead and propose, whether a task fits, whether
//! a cap fits in the pool, and when a project has spent its cap.

use super::money::{DOLLAR, Micros};
use super::work::{GoalId, MemberId, Millis, ProjectId, Refusal, Role, TaskId};

/// Of its cap, the share a project may spend before the owner hears of it, once a month.
pub const WARN_PERCENT: i64 = 80;

/// Less than this left of its cap, and a project has spent it: about what one model call of
/// a briefing costs, so a turn could do nothing with what is left.
pub const SPENT_WITHIN: Micros = 50_000;

/// Where a project is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStatus {
    /// Proposed by a lead, waiting for the CEO.
    Proposed,
    /// Being worked.
    Active,
    /// Held: by the owner, or by its budget until the month turns or the owner raises it.
    Paused,
    /// Reached.
    Done,
    /// Given up, or sent back.
    Dropped,
}

impl ProjectStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    /// The status with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Proposed,
            Self::Active,
            Self::Paused,
            Self::Done,
            Self::Dropped,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }

    /// Whether its cap counts against the pool's month: proposed, active, or paused (a paused
    /// project comes back with its cap).
    #[must_use]
    pub const fn holds_its_cap(self) -> bool {
        matches!(self, Self::Proposed | Self::Active | Self::Paused)
    }

    /// Whether a task may be put in it: active or paused.
    #[must_use]
    pub const fn takes_work(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }
}

/// Why a project is paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// It spent its cap for the month: its Claude turns wait, and the pause lifts itself when
    /// the month turns or the owner raises the cap. Members on the house's model go on.
    Budget,
    /// The owner paused it: every turn on it waits until the owner resumes it.
    Owner,
}

impl Pause {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Budget => "budget",
            Self::Owner => "owner",
        }
    }

    /// The reason with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Budget, Self::Owner]
            .into_iter()
            .find(|p| p.name() == name)
    }
}

/// What a proposal points at in HQ's own record: a task, or an event (an alarm, say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// A task, by its number.
    Task(TaskId),
    /// An event in the feed, by its id.
    Event(i64),
}

impl Evidence {
    /// As the owner and the CEO read it: `task 12`, `event 340`.
    #[must_use]
    pub fn said(self) -> String {
        match self {
            Self::Task(t) => format!("task {t}"),
            Self::Event(e) => format!("event {e}"),
        }
    }

    /// As it is kept: `task:12,event:340`.
    #[must_use]
    pub fn kept(all: &[Self]) -> String {
        all.iter()
            .map(|e| match e {
                Self::Task(t) => format!("task:{t}"),
                Self::Event(id) => format!("event:{id}"),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Read back from how it is kept; what does not read is left out.
    #[must_use]
    pub fn read(kept: &str) -> Vec<Self> {
        kept.split(',')
            .filter_map(|part| {
                let (kind, id) = part.trim().split_once(':')?;
                let id: i64 = id.parse().ok()?;
                match kind {
                    "task" => Some(Self::Task(TaskId(id))),
                    "event" => Some(Self::Event(id)),
                    _ => None,
                }
            })
            .collect()
    }
}

/// A piece of a goal, with its lead and its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Its id.
    pub id: ProjectId,
    /// The one goal it serves.
    pub goal: GoalId,
    /// What it is.
    pub title: String,
    /// Why it is worth doing.
    pub reason: String,
    /// Who leads it: a lead or the CEO.
    pub lead: MemberId,
    /// Where it is.
    pub status: ProjectStatus,
    /// Why it is paused, while it is.
    pub pause: Option<Pause>,
    /// The most it may spend in a calendar month, if the owner set a cap.
    pub cap: Option<Micros>,
    /// A cap asked for and waiting on the owner's desk, if any.
    pub asked_cap: Option<Micros>,
    /// Who made it: `owner`, `endora`, or a member's name.
    pub created_by: String,
    /// The member who proposed it, if a member did.
    pub proposer: Option<MemberId>,
    /// The task the proposal was made in, whose thread hears the CEO's word.
    pub proposed_in: Option<TaskId>,
    /// What in HQ's record the proposal pointed at.
    pub evidence: Vec<Evidence>,
    /// When it was made.
    pub created_at: Millis,
    /// When it last changed.
    pub updated_at: Millis,
}

/// Whether a member in `role` may lead a project.
///
/// # Errors
/// [`Refusal::NotAProjectLead`] for anyone but a lead or the CEO.
pub const fn may_lead(role: Role) -> Result<(), Refusal> {
    if role.leads() {
        return Ok(());
    }
    Err(Refusal::NotAProjectLead)
}

/// Whether a member in `role` may propose a project: a lead, who spots the pattern in its
/// area; the CEO decides proposals and makes none (ADR 0120).
///
/// # Errors
/// [`Refusal::OnlyLeadsPropose`] for anyone but a lead.
pub const fn may_propose(role: Role) -> Result<(), Refusal> {
    if matches!(role, Role::Lead) {
        return Ok(());
    }
    Err(Refusal::OnlyLeadsPropose)
}

/// Whether a task, or a routine, under `goal` may be put in a project serving `serves`, which
/// is now `status`.
///
/// # Errors
/// [`Refusal::OtherGoal`] for a project of another goal; [`Refusal::ProjectClosed`] for one
/// that takes no work.
pub fn may_hold(serves: GoalId, status: ProjectStatus, goal: GoalId) -> Result<(), Refusal> {
    if serves != goal {
        return Err(Refusal::OtherGoal);
    }
    if !status.takes_work() {
        return Err(Refusal::ProjectClosed);
    }
    Ok(())
}

/// Whether a cap fits: above nothing, and with the other projects' caps (`others`, those that
/// [hold their cap](ProjectStatus::holds_its_cap)) no more than the pool's month.
///
/// # Errors
/// [`Refusal::CapsOverPool`] with what room there is.
pub const fn may_cap(cap: Micros, others: Micros, pool_per_month: Micros) -> Result<(), Refusal> {
    let room = pool_per_month - others;
    if cap <= 0 || cap > room {
        let room = if room < 0 { 0 } else { room };
        return Err(Refusal::CapsOverPool { room });
    }
    Ok(())
}

/// Whether a cap raised on a project paused for its budget is more than it spent this month.
///
/// # Errors
/// [`Refusal::CapNotAboveSpend`] otherwise.
pub const fn may_raise(cap: Micros, spent: Micros) -> Result<(), Refusal> {
    if cap <= spent {
        return Err(Refusal::CapNotAboveSpend { spent });
    }
    Ok(())
}

/// How far into its cap a project's month is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Less than [`WARN_PERCENT`] of it.
    Under,
    /// [`WARN_PERCENT`] of it or more: the owner hears, once a month.
    Warned,
    /// All of it, within [`SPENT_WITHIN`]: its Claude turns wait.
    Spent,
}

/// How far `spent` is into `cap`.
#[must_use]
pub const fn level(spent: Micros, cap: Micros) -> Level {
    if cap - spent < SPENT_WITHIN {
        return Level::Spent;
    }
    if spent * 100 >= cap * WARN_PERCENT {
        return Level::Warned;
    }
    Level::Under
}

/// What a project may still commit this month: its cap less what it committed, and nothing
/// once less than [`SPENT_WITHIN`] is left.
#[must_use]
pub const fn left(committed: Micros, cap: Micros) -> Micros {
    let left = cap - committed;
    if left < SPENT_WITHIN {
        return 0;
    }
    left
}

const DAY_MS: Millis = 86_400_000;

/// Days since the epoch of a civil date (Howard Hinnant's days-from-civil).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The calendar month in UTC a moment falls in, as `year * 12 + month - 1`.
#[must_use]
pub const fn month_of(at: Millis) -> i64 {
    let z = at.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    year * 12 + month - 1
}

/// The first moment of a month (as [`month_of`] counts them), in UTC.
#[must_use]
pub const fn month_start(month: i64) -> Millis {
    days_from_civil(month.div_euclid(12), month.rem_euclid(12) + 1, 1) * DAY_MS
}

/// The month `at` falls in, as the moments it runs from (inclusive) and to (exclusive).
#[must_use]
pub const fn month_around(at: Millis) -> (Millis, Millis) {
    let month = month_of(at);
    (month_start(month), month_start(month + 1))
}

/// An amount as dollars and cents: `$12.50`.
#[must_use]
pub fn dollars(amount: Micros) -> String {
    let cents = (amount + 5_000).div_euclid(10_000);
    format!("${}.{:02}", cents.div_euclid(100), cents.rem_euclid(100))
}

/// An amount the owner or the CEO wrote in dollars (`25`, `$12.50`), if it reads as one
/// above nothing, to the cent.
#[must_use]
pub fn read_dollars(text: &str) -> Option<Micros> {
    let text = text.trim().trim_start_matches('$').trim();
    let (whole, cents) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && cents.is_empty() {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    if cents.len() > 2 || !cents.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let cents: i64 = format!("{cents:0<2}").parse().ok()?;
    let amount = whole.checked_mul(DOLLAR)?.checked_add(cents * 10_000)?;
    (amount > 0).then_some(amount)
}

/// The tools a project's work is given, when the owner narrows them (the Holodeck plan's phase
/// 4): kept as one setting a project, a JSON list of tool names, or `null` for none. A turn on
/// the project's work is given these and its own four ([`super::deploy::HQ_TOOLS`]) alone;
/// with no list, what its role and brain give it, as before.
#[must_use]
pub fn tools_key(id: ProjectId) -> String {
    format!("project_tools:{}", id.0)
}

/// A project's tools, as [`tools_key`] keeps them: `None` for no list.
#[must_use]
pub fn tools_read(kept: Option<&str>) -> Option<Vec<String>> {
    serde_json::from_str::<Option<Vec<String>>>(kept?)
        .ok()
        .flatten()
}

/// The workbench's shell, by the name the worker offers it: given only when a project's list
/// names it.
pub const SHELL: &str = "shell";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::money::POOL_PER_MONTH;

    #[test]
    fn a_lead_or_the_ceo_leads_a_project_and_only_a_lead_proposes_one() {
        for role in [Role::Ceo, Role::Lead] {
            assert!(may_lead(role).is_ok(), "{role:?}");
        }
        // Leads propose; the CEO decides (ADR 0120).
        assert!(may_propose(Role::Lead).is_ok());
        assert_eq!(may_propose(Role::Ceo), Err(Refusal::OnlyLeadsPropose));
        for role in [Role::Developer, Role::Researcher, Role::Reviewer] {
            assert_eq!(may_lead(role), Err(Refusal::NotAProjectLead));
            assert_eq!(may_propose(role), Err(Refusal::OnlyLeadsPropose));
        }
    }

    #[test]
    fn a_task_goes_only_in_a_project_of_its_own_goal_that_takes_work() {
        let (ours, theirs) = (GoalId(1), GoalId(2));
        assert!(may_hold(ours, ProjectStatus::Active, ours).is_ok());
        assert!(may_hold(ours, ProjectStatus::Paused, ours).is_ok());
        assert_eq!(
            may_hold(theirs, ProjectStatus::Active, ours),
            Err(Refusal::OtherGoal)
        );
        for closed in [
            ProjectStatus::Proposed,
            ProjectStatus::Done,
            ProjectStatus::Dropped,
        ] {
            assert_eq!(may_hold(ours, closed, ours), Err(Refusal::ProjectClosed));
        }
    }

    #[test]
    fn the_caps_together_never_pass_the_pools_month() {
        assert!(may_cap(20 * DOLLAR, 50 * DOLLAR, POOL_PER_MONTH).is_ok());
        assert_eq!(
            may_cap(21 * DOLLAR, 50 * DOLLAR, POOL_PER_MONTH),
            Err(Refusal::CapsOverPool { room: 20 * DOLLAR })
        );
        assert_eq!(
            may_cap(0, 0, POOL_PER_MONTH),
            Err(Refusal::CapsOverPool {
                room: POOL_PER_MONTH
            })
        );
        assert_eq!(
            Refusal::CapsOverPool { room: 20 * DOLLAR }.to_string(),
            "the projects' caps together may not pass the pool's month: at most $20.00 is left \
             for this one"
        );
        assert_eq!(
            may_raise(5 * DOLLAR, 5 * DOLLAR),
            Err(Refusal::CapNotAboveSpend { spent: 5 * DOLLAR })
        );
        assert!(may_raise(6 * DOLLAR, 5 * DOLLAR).is_ok());
    }

    #[test]
    fn a_project_is_warned_at_eighty_percent_and_spent_within_five_cents_of_its_cap() {
        let cap = 10 * DOLLAR;
        assert_eq!(level(7_999_999, cap), Level::Under);
        assert_eq!(level(8 * DOLLAR, cap), Level::Warned);
        assert_eq!(level(cap - SPENT_WITHIN, cap), Level::Warned);
        assert_eq!(level(cap - SPENT_WITHIN + 1, cap), Level::Spent);
        assert_eq!(level(cap + 1, cap), Level::Spent);
        assert_eq!(left(4 * DOLLAR, cap), 6 * DOLLAR);
        assert_eq!(left(cap - 10_000, cap), 0, "less than a call is left");
    }

    #[test]
    fn a_month_is_the_calendar_month_in_utc() {
        // 17 October 2026, at noon UTC.
        let at = 1_792_238_400_000;
        let (from, until) = month_around(at);
        assert_eq!(from, 1_790_812_800_000, "1 October 2026, 00:00 UTC");
        assert_eq!(until, 1_793_491_200_000, "1 November 2026, 00:00 UTC");
        assert_eq!(month_of(until - 1) + 1, month_of(until));
        assert_eq!(month_of(at), 2026 * 12 + 9);
        // December rolls into January of the next year.
        let december = month_start(2026 * 12 + 11);
        assert_eq!(month_of(december), 2026 * 12 + 11);
        assert_eq!(month_start(2027 * 12), month_around(december).1);
    }

    #[test]
    fn dollars_read_and_are_said_to_the_cent() {
        assert_eq!(dollars(12_500_000), "$12.50");
        assert_eq!(dollars(4_999), "$0.00");
        assert_eq!(dollars(5_000), "$0.01");
        assert_eq!(read_dollars("25"), Some(25 * DOLLAR));
        assert_eq!(read_dollars(" $12.5 "), Some(12_500_000));
        assert_eq!(read_dollars("0.05"), Some(50_000));
        for bad in ["", "$", "0", "-3", "1.234", "ten", "1.x"] {
            assert_eq!(read_dollars(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn evidence_is_kept_and_read_back() {
        let all = [Evidence::Task(TaskId(12)), Evidence::Event(340)];
        let kept = Evidence::kept(&all);
        assert_eq!(kept, "task:12,event:340");
        assert_eq!(Evidence::read(&kept), all);
        assert_eq!(Evidence::read("task:x,nonsense"), []);
        assert_eq!(all.map(Evidence::said), ["task 12", "event 340"]);
        for s in [
            ProjectStatus::Proposed,
            ProjectStatus::Active,
            ProjectStatus::Paused,
            ProjectStatus::Done,
            ProjectStatus::Dropped,
        ] {
            assert_eq!(ProjectStatus::from_name(s.name()), Some(s));
        }
        assert_eq!(Pause::from_name("budget"), Some(Pause::Budget));
        assert_eq!(Pause::from_name("owner"), Some(Pause::Owner));
    }
}
