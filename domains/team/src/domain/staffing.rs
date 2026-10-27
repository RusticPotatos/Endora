//! Which lead a member works under, for the map's area pages (ADR 0118, amended by ADR 0142:
//! furlough and recall retired with staffing).

use super::work::{Member, MemberId, Role};

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
