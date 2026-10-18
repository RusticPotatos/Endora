//! Elastic staffing (ADR 0118): the team grows and shrinks with its work, and nobody is ever
//! fired. A member with nothing to do for a week is offered to the owner for **furlough**; a
//! lead with a backlog asks for staff and gets a furloughed member back before anyone is
//! hired. Both directions are the owner's to approve. The rules here are pure: the engine
//! gathers the record and asks them.

use super::work::{Member, MemberId, Millis, Refusal, Role};

/// How long a member has to have had nothing to do before it is offered for furlough.
pub const IDLE_AFTER_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// How long after the owner keeps a member before it is offered again.
pub const KEPT_FOR_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// How many open tasks of a project a request for staff must cite for each active developer
/// the project already has.
pub const BACKLOG_PER_DEVELOPER: usize = 2;

/// Whether a member may be offered for furlough at all: not the CEO, not a lead, and not
/// already on furlough.
#[must_use]
pub const fn may_be_offered(member: &Member) -> bool {
    !matches!(member.role, Role::Ceo | Role::Lead) && !member.furloughed()
}

/// Since when a member has had nothing to do, if it has had nothing for a week: `baseline`
/// is when the team first saw it (or when it was last recalled), `last_active` the latest
/// moment it was woken, assigned something or took a turn. `None` when it is not idle.
#[must_use]
pub fn idle_since(baseline: Millis, last_active: Option<Millis>, now: Millis) -> Option<Millis> {
    let since = last_active.map_or(baseline, |at| at.max(baseline));
    (now - since >= IDLE_AFTER_MS).then_some(since)
}

/// How many open tasks of a project a request for staff must cite, given how many active
/// developers the project has: [`BACKLOG_PER_DEVELOPER`] each, and at least one.
#[must_use]
pub fn backlog_needed(developers: usize) -> usize {
    (developers * BACKLOG_PER_DEVELOPER).max(1)
}

/// Whether `cited` open tasks of a project are backlog enough for it to have more staff.
///
/// # Errors
/// [`Refusal::NoBacklog`], naming the count.
pub fn may_ask(cited: usize, developers: usize) -> Result<(), Refusal> {
    let needs = backlog_needed(developers);
    if cited >= needs {
        return Ok(());
    }
    Err(Refusal::NoBacklog {
        cited,
        developers,
        needs,
    })
}

/// The role a request for staff is for: a developer (the lead's backlog is of code).
pub const STAFF_ROLE: Role = Role::Developer;

/// The furloughed member a request for staff brings back, if there is one of the role: the
/// lead's own first, else the one furloughed longest.
#[must_use]
pub fn to_recall(members: &[Member], role: Role, lead: MemberId) -> Option<MemberId> {
    let mut out: Vec<&Member> = members
        .iter()
        .filter(|m| m.role == role && m.furloughed())
        .collect();
    out.sort_by_key(|m| (m.reports_to != Some(lead), m.furloughed_at, m.id));
    out.first().map(|m| m.id)
}

/// The offer of a furlough, as it is written on the desk.
#[must_use]
pub fn offer_text(name: &str, since: &str) -> String {
    format!(
        "{name} has had nothing to do since {since}: furlough, or keep? Furloughed, {name} is \
         never woken and costs nothing, and keeps its record, its name and its notebook; \
         nobody is let go."
    )
}

/// A recall a lead asks the owner for, as it is written on the desk and read back from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallRequest {
    /// The furloughed member to bring back.
    pub member: MemberId,
    /// Whom it will report to: the lead who asked.
    pub reports_to: MemberId,
    /// Why, in the lead's words.
    pub why: String,
}

impl RecallRequest {
    /// The desk item's text: the first line is what [`Self::read`] reads back; the rest is for
    /// the owner.
    #[must_use]
    pub fn write(&self, name: &str, asker: &str, evidence: &str) -> String {
        format!(
            "Recall member {} to report to member {}.\n{name} is on furlough; {asker} asks: \
             {}\nEvidence: {evidence}",
            self.member.0,
            self.reports_to.0,
            self.why.trim()
        )
    }

    /// The request from a desk item's text, if it is one.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let first = text.lines().next()?.strip_prefix("Recall member ")?;
        let (member, rest) = first.split_once(" to report to member ")?;
        let to = rest.strip_suffix('.')?.parse::<i64>().ok()?;
        let why = text
            .lines()
            .nth(1)
            .and_then(|l| l.split_once(" asks: "))
            .map_or("", |(_, why)| why);
        Some(Self {
            member: MemberId(member.parse().ok()?),
            reports_to: MemberId(to),
            why: why.to_owned(),
        })
    }
}

/// The lead a member works under: the nearest lead up whom it reports to, or `None` when the
/// chain reaches the CEO (or nobody) first. A lead is its own.
#[must_use]
pub fn lead_of(member: &Member, members: &[Member]) -> Option<MemberId> {
    if member.role == Role::Lead {
        return Some(member.id);
    }
    let mut seen = vec![member.id];
    let mut at = member;
    loop {
        let up = super::chain::manager(at, members)?;
        if seen.contains(&up) {
            return None;
        }
        seen.push(up);
        let found = members.iter().find(|m| m.id == up)?;
        match found.role {
            Role::Lead => return Some(found.id),
            Role::Ceo => return None,
            _ => at = found,
        }
    }
}

/// One team's seats, for the scorecard: the lead and whoever works under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamSeats {
    /// The team's lead.
    pub lead: MemberId,
    /// Every member of the team, the lead included, furloughed or not.
    pub seats: usize,
    /// The members at work.
    pub active: usize,
    /// The members on furlough.
    pub furloughed: usize,
}

/// Each lead's team, in the order the leads joined.
#[must_use]
pub fn teams(members: &[Member]) -> Vec<TeamSeats> {
    members
        .iter()
        .filter(|m| m.role == Role::Lead)
        .map(|lead| {
            let team: Vec<&Member> = members
                .iter()
                .filter(|m| lead_of(m, members) == Some(lead.id))
                .collect();
            let furloughed = team.iter().filter(|m| m.furloughed()).count();
            TeamSeats {
                lead: lead.id,
                seats: team.len(),
                active: team.len() - furloughed,
                furloughed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Brain;

    fn member(id: i64, role: Role, to: Option<i64>, furloughed: bool) -> Member {
        Member {
            id: MemberId(id),
            name: format!("m{id}"),
            role,
            share_percent: role.share(),
            slots: 1,
            look: 0,
            paused: false,
            brain: Brain::Local,
            reports_to: to.map(MemberId),
            furloughed_at: furloughed.then_some(5),
            recalled_at: None,
        }
    }

    #[test]
    fn the_ceo_a_lead_and_a_member_already_out_are_never_offered() {
        assert!(!may_be_offered(&member(1, Role::Ceo, None, false)));
        assert!(!may_be_offered(&member(2, Role::Lead, Some(1), false)));
        assert!(!may_be_offered(&member(3, Role::Developer, Some(2), true)));
        assert!(may_be_offered(&member(3, Role::Developer, Some(2), false)));
        assert!(may_be_offered(&member(4, Role::Reviewer, Some(2), false)));
    }

    #[test]
    fn a_member_is_idle_after_a_week_of_nothing_counted_from_the_latest_thing() {
        let day = 24 * 60 * 60 * 1000;
        assert_eq!(idle_since(0, None, 7 * day), Some(0));
        assert_eq!(idle_since(0, None, 7 * day - 1), None);
        assert_eq!(idle_since(0, Some(3 * day), 9 * day), None);
        assert_eq!(idle_since(0, Some(3 * day), 10 * day), Some(3 * day));
        // A recall (the baseline) after the last turn starts the week again.
        assert_eq!(idle_since(5 * day, Some(day), 9 * day), None);
    }

    #[test]
    fn a_request_for_staff_cites_twice_the_developers_in_open_tasks() {
        assert_eq!(backlog_needed(0), 1);
        assert_eq!(backlog_needed(3), 6);
        assert_eq!(may_ask(6, 3), Ok(()));
        assert_eq!(
            may_ask(5, 3),
            Err(Refusal::NoBacklog {
                cited: 5,
                developers: 3,
                needs: 6
            })
        );
    }

    #[test]
    fn the_leads_own_furloughed_developer_comes_back_first() {
        let team = [
            member(1, Role::Ceo, None, false),
            member(2, Role::Lead, Some(1), false),
            member(3, Role::Lead, Some(1), false),
            member(4, Role::Developer, Some(3), true),
            member(5, Role::Developer, Some(2), true),
            member(6, Role::Reviewer, Some(2), true),
        ];
        assert_eq!(
            to_recall(&team, Role::Developer, MemberId(2)),
            Some(MemberId(5))
        );
        assert_eq!(
            to_recall(&team, Role::Developer, MemberId(3)),
            Some(MemberId(4))
        );
        assert_eq!(to_recall(&team[..3], Role::Developer, MemberId(2)), None);
    }

    #[test]
    fn a_recall_is_written_and_read_back_from_the_desk() {
        let request = RecallRequest {
            member: MemberId(7),
            reports_to: MemberId(4),
            why: "four branches wait".to_owned(),
        };
        let text = request.write("Rae", "Kit", "task 3, task 4");
        assert_eq!(RecallRequest::read(&text), Some(request));
        assert_eq!(
            RecallRequest::read("Hire Sam as developer on local, reporting to member 4."),
            None
        );
    }

    #[test]
    fn each_leads_team_counts_its_seats_active_and_furloughed() {
        let team = [
            member(1, Role::Ceo, None, false),
            member(2, Role::Lead, Some(1), false),
            member(3, Role::Developer, Some(2), false),
            member(4, Role::Developer, Some(2), true),
            member(5, Role::Reviewer, Some(1), false),
        ];
        assert_eq!(
            teams(&team),
            [TeamSeats {
                lead: MemberId(2),
                seats: 3,
                active: 2,
                furloughed: 1
            }]
        );
        assert_eq!(lead_of(&team[4], &team), None);
    }
}
