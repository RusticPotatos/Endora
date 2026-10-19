//! Consulting a member (ADR 0129): a member asks another a question directly, and the other
//! answers in one short turn, or, when it cannot be woken, the answer is found in the record.
//!
//! The rules here are the owner's, enforced in code: any member may consult any other, but the
//! CEO consults leads only (ADR 0120 keeps it out of the weeds); at most two consults in a turn
//! and six in a member's day; a member who is paused or on furlough is never woken (ADR 0118);
//! and the owner is not consulted this way, only through `ask_owner`.

use super::history::HistoryRefusal;
use super::work::{Member, MemberId, Millis, Role, RunId, TaskId};

/// The most consults one turn makes.
pub const PER_TURN: usize = 2;

/// The most consults one member makes in a day.
pub const PER_DAY: usize = 6;

/// The most rounds a consulted member's turn takes before it must answer.
pub const ROUNDS: usize = 6;

/// The most characters of a consult's answer.
pub const ANSWER_MOST: usize = 1_200;

/// Where a consult is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Its member was woken and has not answered.
    Queued,
    /// Its member answered.
    Answered,
    /// Its member was not woken, or did not answer: the record answered.
    FromRecord,
}

impl Status {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Answered => "answered",
            Self::FromRecord => "from_record",
        }
    }

    /// The status with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Queued, Self::Answered, Self::FromRecord]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// One consult, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consult {
    /// Its row; 0 before it is kept.
    pub id: i64,
    /// Who asked.
    pub asker: MemberId,
    /// The turn it asked in.
    pub asker_run: RunId,
    /// Who was asked.
    pub expert: MemberId,
    /// The task the question is about: what the expert's handover notes and last words are read
    /// on.
    pub topic: Option<TaskId>,
    /// The task whose thread the answer goes in.
    pub thread: TaskId,
    /// The question, cleaned.
    pub question: String,
    /// Where it is.
    pub status: Status,
    /// The turn that answered it, once the expert's turn began.
    pub answer_run: Option<RunId>,
    /// Why it was answered from the record, if it was.
    pub why: String,
    /// Whether the asker has been woken to read the answer.
    pub asker_woken: bool,
    /// When it was asked.
    pub at: Millis,
}

/// Why an expert cannot be woken, in a few words, if it cannot: paused by the owner, or on
/// furlough.
#[must_use]
pub fn cannot_be_woken(expert: &Member) -> Option<&'static str> {
    if expert.paused {
        return Some("is paused");
    }
    if expert.furloughed() {
        return Some("is on furlough");
    }
    None
}

/// Whether `asker` may consult `expert` now: not itself, the CEO only a lead, and within the
/// limits (`this_turn` consults made in this turn, `today` in this day, by the asker).
///
/// # Errors
/// The refusal that says which rule, so the asker can do something about it.
pub fn check(
    asker: &Member,
    expert: &Member,
    this_turn: usize,
    today: usize,
) -> Result<(), HistoryRefusal> {
    if asker.id == expert.id {
        return Err(HistoryRefusal::NotYourself);
    }
    if asker.role == Role::Ceo && expert.role != Role::Lead {
        return Err(HistoryRefusal::CeoConsultsLeads);
    }
    if this_turn >= PER_TURN {
        return Err(HistoryRefusal::TooManyThisTurn);
    }
    if today >= PER_DAY {
        return Err(HistoryRefusal::TooManyToday);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::brain::Brain;

    fn member(id: i64, role: Role) -> Member {
        Member {
            id: MemberId(id),
            name: format!("Member {id}"),
            role,
            share_percent: 25,
            slots: 2,
            look: 0,
            paused: false,
            brain: Brain::Local,
            work_brain: Brain::Local,
            reports_to: None,
            furloughed_at: None,
            recalled_at: None,
        }
    }

    #[test]
    fn any_member_may_consult_any_other_within_the_limits() {
        let dev = member(1, Role::Developer);
        for role in [
            Role::Lead,
            Role::Developer,
            Role::Reviewer,
            Role::Researcher,
            Role::Ceo,
        ] {
            assert_eq!(check(&dev, &member(2, role), 0, 0), Ok(()), "{role:?}");
        }
    }

    #[test]
    fn nobody_consults_itself_and_the_ceo_consults_leads_only() {
        let ceo = member(1, Role::Ceo);
        assert_eq!(check(&ceo, &ceo, 0, 0), Err(HistoryRefusal::NotYourself));
        assert_eq!(check(&ceo, &member(2, Role::Lead), 0, 0), Ok(()));
        for role in [Role::Developer, Role::Reviewer, Role::Researcher] {
            assert_eq!(
                check(&ceo, &member(2, role), 0, 0),
                Err(HistoryRefusal::CeoConsultsLeads)
            );
        }
    }

    #[test]
    fn two_a_turn_and_six_a_day() {
        let (a, b) = (member(1, Role::Developer), member(2, Role::Developer));
        assert_eq!(check(&a, &b, 1, 5), Ok(()));
        assert_eq!(check(&a, &b, 2, 0), Err(HistoryRefusal::TooManyThisTurn));
        assert_eq!(check(&a, &b, 0, 6), Err(HistoryRefusal::TooManyToday));
    }

    #[test]
    fn a_paused_or_furloughed_member_cannot_be_woken() {
        let mut m = member(1, Role::Developer);
        assert_eq!(cannot_be_woken(&m), None);
        m.paused = true;
        assert_eq!(cannot_be_woken(&m), Some("is paused"));
        m.paused = false;
        m.furloughed_at = Some(5);
        assert_eq!(cannot_be_woken(&m), Some("is on furlough"));
        m.recalled_at = Some(9);
        assert_eq!(cannot_be_woken(&m), None);
    }

    #[test]
    fn a_status_is_read_back() {
        for s in [Status::Queued, Status::Answered, Status::FromRecord] {
            assert_eq!(Status::from_name(s.name()), Some(s));
        }
        assert_eq!(Status::from_name("x"), None);
    }
}
