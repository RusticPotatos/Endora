//! The escalation ladder (ADR 0120, the owner's amendment of 2026-10-18): stuck work climbs
//! one rung at a time, each rung once, instead of landing on the owner's desk.
//!
//! 1. **The lead**: the lead of the task's area is woken to decide.
//! 2. **A huddle**: the lead and up to [`HUDDLE_MOST`] members who touched the task in the last
//!    [`TOUCHED_WITHIN_MS`] each take one short turn to give their view; then the lead decides.
//! 3. **The CEO**: only when the lead's decision says it needs the CEO (money, a new goal,
//!    another area), or the lead did not decide.
//! 4. **The board**: only a board matter (the lead says so), through the CEO when there is one.
//!
//! Live, task #666 went in circles for hours and then sat on the owner's desk as "stuck".

use super::work::{MemberId, Millis, Refusal};

/// How many members besides the lead a huddle hears.
pub const HUDDLE_MOST: usize = 4;
/// How recently a member must have touched a task to be in its huddle: two days.
pub const TOUCHED_WITHIN_MS: Millis = 48 * 60 * 60 * 1000;
/// How long a huddle waits for its views before its lead decides anyway.
pub const VIEWS_WAIT_MS: Millis = 30 * 60 * 1000;
/// The most characters a view may run to.
pub const VIEW_MOST: usize = 600;
/// The most sentences a view may run to.
pub const VIEW_SENTENCES: usize = 3;

/// Where a stuck task is on the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// Its area's lead was asked to decide.
    Lead,
    /// A huddle was called.
    Huddle,
    /// It went to the CEO.
    Ceo,
    /// Its lead decided.
    Decided,
}

impl Rung {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Huddle => "huddle",
            Self::Ceo => "ceo",
            Self::Decided => "decided",
        }
    }

    /// The rung with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Lead, Self::Huddle, Self::Ceo, Self::Decided]
            .into_iter()
            .find(|r| r.name() == name)
    }
}

/// The rung a task climbs to when it is found stuck, from where it is (`None`: not on the
/// ladder) and whether its work moved since. Work that moved, or a decision that was carried
/// out and stuck again, starts again at the lead. After the CEO there is no rung left: nothing
/// more happens, and the owner's desk is never the next step (`None`).
#[must_use]
pub const fn climb(at: Option<Rung>, moved_since: bool) -> Option<Rung> {
    match (at, moved_since) {
        (None | Some(Rung::Decided), _) | (Some(_), true) => Some(Rung::Lead),
        (Some(Rung::Lead), false) => Some(Rung::Huddle),
        (Some(Rung::Huddle), false) => Some(Rung::Ceo),
        (Some(Rung::Ceo), false) => None,
    }
}

/// Who a huddle hears besides its lead: the members who touched the task within
/// [`TOUCHED_WITHIN_MS`] of `now` (each `(member, when)`), newest first, each once, never the
/// lead or anyone in `left_out`, at most [`HUDDLE_MOST`].
#[must_use]
pub fn attendees(
    touched: &[(MemberId, Millis)],
    lead: MemberId,
    left_out: &[MemberId],
    now: Millis,
) -> Vec<MemberId> {
    let mut recent: Vec<&(MemberId, Millis)> = touched
        .iter()
        .filter(|(m, at)| now - at <= TOUCHED_WITHIN_MS && *m != lead && !left_out.contains(m))
        .collect();
    recent.sort_by_key(|(_, at)| core::cmp::Reverse(*at));
    let mut chosen: Vec<MemberId> = Vec::new();
    for (member, _) in recent {
        if !chosen.contains(member) {
            chosen.push(*member);
        }
    }
    chosen.truncate(HUDDLE_MOST);
    chosen
}

/// A member's view for a huddle as kept: one line, at most [`VIEW_SENTENCES`] sentences and
/// [`VIEW_MOST`] characters.
///
/// # Errors
/// [`Refusal::EmptyNote`] for no words; [`Refusal::TooLong`] past its characters;
/// [`Refusal::ViewTooLong`] past its sentences.
pub fn view(text: &str) -> Result<String, Refusal> {
    let kept = super::evidence::short(text, VIEW_MOST + 1);
    if kept.is_empty() {
        return Err(Refusal::EmptyNote);
    }
    if kept.chars().count() > VIEW_MOST {
        return Err(Refusal::TooLong(VIEW_MOST));
    }
    let sentences = kept
        .split(['.', '!', '?'])
        .filter(|s| !s.trim().is_empty())
        .count();
    if sentences > VIEW_SENTENCES {
        return Err(Refusal::ViewTooLong);
    }
    Ok(kept)
}

/// What a lead decides about stuck work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Go on, as it says how.
    Proceed,
    /// Give it up.
    Drop,
    /// Hand it to another member of the lead's team.
    HandTo,
    /// It needs the CEO: money, a new goal, another area.
    NeedsCeo,
    /// It needs the board: money beyond the caps, a new goal, a hire, the owner's repos.
    NeedsBoard,
}

impl Decision {
    /// Stable name, as the tool takes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proceed => "proceed",
            Self::Drop => "drop",
            Self::HandTo => "hand_to",
            Self::NeedsCeo => "needs_ceo",
            Self::NeedsBoard => "needs_board",
        }
    }

    /// The decision with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Proceed,
            Self::Drop,
            Self::HandTo,
            Self::NeedsCeo,
            Self::NeedsBoard,
        ]
        .into_iter()
        .find(|d| d.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuck_work_climbs_one_rung_at_a_time_and_never_past_the_ceo() {
        assert_eq!(climb(None, false), Some(Rung::Lead));
        assert_eq!(climb(Some(Rung::Lead), false), Some(Rung::Huddle));
        assert_eq!(climb(Some(Rung::Huddle), false), Some(Rung::Ceo));
        assert_eq!(
            climb(Some(Rung::Ceo), false),
            None,
            "the desk is not a rung"
        );
        assert_eq!(
            climb(Some(Rung::Huddle), true),
            Some(Rung::Lead),
            "work that moved starts again"
        );
        assert_eq!(climb(Some(Rung::Decided), false), Some(Rung::Lead));
        for rung in [Rung::Lead, Rung::Huddle, Rung::Ceo, Rung::Decided] {
            assert_eq!(Rung::from_name(rung.name()), Some(rung));
        }
    }

    #[test]
    fn a_huddle_hears_the_lead_and_at_most_four_who_touched_it_lately() {
        let now = 10 * TOUCHED_WITHIN_MS;
        let m = MemberId;
        let touched = [
            (m(2), now - 10),
            (m(3), now - 20),
            (m(3), now - 5),
            (m(4), now - TOUCHED_WITHIN_MS - 1),
            (m(5), now - 30),
            (m(6), now - 40),
            (m(7), now - 50),
            (m(8), now - 60),
            (m(1), now),
        ];
        assert_eq!(
            attendees(&touched, m(1), &[m(8)], now),
            [m(3), m(2), m(5), m(6)],
            "newest first, the lead and the old left out, four at most"
        );
    }

    #[test]
    fn a_view_is_three_sentences_at_most() {
        assert_eq!(
            view("Gluetun restarts. The three follow it."),
            Ok("Gluetun restarts. The three follow it.".to_owned())
        );
        assert_eq!(view("  "), Err(Refusal::EmptyNote));
        assert_eq!(view("One. Two. Three. Four."), Err(Refusal::ViewTooLong));
        assert_eq!(
            view(&"x".repeat(VIEW_MOST + 1)),
            Err(Refusal::TooLong(VIEW_MOST))
        );
        for d in ["proceed", "drop", "hand_to", "needs_ceo", "needs_board"] {
            assert_eq!(Decision::from_name(d).map(Decision::name), Some(d));
        }
    }
}
