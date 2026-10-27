//! Elastic staffing (ADR 0118): the team grows and shrinks with its work, and nobody is ever
//! fired. A member with nothing to do for a week is offered to the owner for **furlough**; a
//! lead with a backlog asks for staff and gets a furloughed member back before anyone is
//! hired. Both directions are the owner's to approve. The rules here are pure: the engine
//! gathers the record and asks them.

use super::work::{Member, MemberId, Millis, Role};

/// How long a member has to have had nothing to do before it is offered for furlough.
pub const IDLE_AFTER_MS: Millis = 7 * 24 * 60 * 60 * 1000;

/// How many open tasks of a project a request for staff must cite for each active developer
/// the project already has.
pub const BACKLOG_PER_DEVELOPER: usize = 2;

/// How many open tasks of a project a request for staff must cite, given how many active
/// developers the project has: [`BACKLOG_PER_DEVELOPER`] each, and at least one.
#[must_use]
pub fn backlog_needed(developers: usize) -> usize {
    (developers * BACKLOG_PER_DEVELOPER).max(1)
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
            brain: Brain::Haiku,
            work_brain: Brain::Mac,
            reports_to: to.map(MemberId),
            furloughed_at: furloughed.then_some(5),
            recalled_at: None,
        }
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
