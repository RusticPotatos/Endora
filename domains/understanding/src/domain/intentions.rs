//! Intentions — what Endora is currently pursuing, and where it got to (ADR 0036).
//!
//! Endora's own working memory, not a task list. Four constraints separate the two, and
//! three of them live here:
//!
//! - an intention **must trace to a belief** ([`Intention::motivating_belief`] is a
//!   [`BeliefId`], never optional), so Endora cannot pursue what it cannot explain;
//! - it **retires itself**, by step budget or staleness, so nothing rots;
//! - progress is captured as the butler's own prose, not a state machine the model has
//!   to maintain.
//!
//! The fourth — **at most one active at a time** — is an application rule, since it is
//! about the set rather than the record.
//!
//! The person cannot create or edit one. Their only verb is to drop it, which is
//! [`abandon`](Intention::abandon).

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{BeliefId, IntentionId, Timestamp};

/// Nights of work before an intention gives up on itself. Seven nights of looking into
/// something without conclusion is Endora's own answer that it is not getting anywhere.
const STEP_BUDGET: u32 = 7;

/// How long without progress before an intention goes stale — what happens when the
/// nightly loop is off or its schedule lapses.
const STALE_AFTER_MS: i64 = 14 * 24 * 60 * 60 * 1_000;

/// Where an intention is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentionState {
    /// Being pursued. At most one intention is in this state at a time (ADR 0036).
    Active,
    /// Endora saw it through.
    Done,
    /// Dropped — by the person, by the step budget, or by going stale. Never by
    /// accumulating quietly.
    Abandoned,
}

impl IntentionState {
    /// A stable, lowercase name for storage and interfaces. The round trip with
    /// [`from_name`](Self::from_name) is part of the contract.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Abandoned => "abandoned",
        }
    }

    /// Parses a state from its [`name`](Self::name); unknown values read as
    /// [`Abandoned`](Self::Abandoned) so a corrupt row can never resurrect itself into
    /// something Endora keeps working on.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "active" => Self::Active,
            "done" => Self::Done,
            _ => Self::Abandoned,
        }
    }
}

/// One thing Endora is pursuing on the person's behalf, and why (ADR 0036).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intention {
    id: IntentionId,
    statement: String,
    motivating_belief: BeliefId,
    note: String,
    state: IntentionState,
    created_at: Timestamp,
    last_progressed_at: Timestamp,
    steps_taken: u32,
}

impl Intention {
    /// Forms an intention from a belief. Endora does this; the person cannot.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `statement` is blank — an intention that names
    /// nothing cannot be pursued or explained.
    pub fn form(
        id: IntentionId,
        statement: &str,
        motivating_belief: BeliefId,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let statement = require_non_empty("intention.statement", statement)?;
        Ok(Self {
            id,
            statement,
            motivating_belief,
            note: String::new(),
            state: IntentionState::Active,
            created_at: at,
            last_progressed_at: at,
            steps_taken: 0,
        })
    }

    /// Reconstitutes a stored intention (all fields explicit).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn from_parts(
        id: IntentionId,
        statement: String,
        motivating_belief: BeliefId,
        note: String,
        state: IntentionState,
        created_at: Timestamp,
        last_progressed_at: Timestamp,
        steps_taken: u32,
    ) -> Self {
        Self {
            id,
            statement,
            motivating_belief,
            note,
            state,
            created_at,
            last_progressed_at,
            steps_taken,
        }
    }

    /// Records a night's work: the butler's own account becomes the note it will be
    /// handed next time, and the step budget ticks down.
    ///
    /// Deliberately takes prose rather than structured state. The model is the least
    /// reliable component in the system, and this is state that must not corrupt
    /// (ADR 0036).
    pub fn progress(&mut self, note: &str, at: Timestamp) {
        self.note = note.trim().to_owned();
        self.last_progressed_at = at;
        self.steps_taken = self.steps_taken.saturating_add(1);
    }

    /// Endora saw it through.
    pub const fn complete(&mut self) {
        self.state = IntentionState::Done;
    }

    /// Dropped — by the person, or by [`retire_if_over`](Self::retire_if_over).
    pub const fn abandon(&mut self) {
        self.state = IntentionState::Abandoned;
    }

    /// Whether it has spent its step budget.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.steps_taken >= STEP_BUDGET
    }

    /// Whether nothing has happened for long enough that it should stop counting as
    /// something Endora is doing. A clock that goes backwards ages nothing.
    #[must_use]
    pub fn is_stale(&self, now: Timestamp) -> bool {
        now.unix_millis()
            .saturating_sub(self.last_progressed_at.unix_millis())
            >= STALE_AFTER_MS
    }

    /// Retires an active intention that is spent or stale, so nothing rots and nothing
    /// waits for the person to close it. Returns why, if it retired.
    pub fn retire_if_over(&mut self, now: Timestamp) -> Option<&'static str> {
        if self.state != IntentionState::Active {
            return None;
        }
        // Exhaustion is checked first: a spent intention that is *also* stale is more
        // usefully explained as "tried seven times" than "nobody ran the loop".
        if self.is_exhausted() {
            self.abandon();
            return Some("gave it seven nights without getting anywhere");
        }
        if self.is_stale(now) {
            self.abandon();
            return Some("hadn't made progress on it in a fortnight");
        }
        None
    }

    /// Whether Endora is still pursuing this.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.state, IntentionState::Active)
    }

    /// The intention's identifier.
    #[must_use]
    pub const fn id(&self) -> IntentionId {
        self.id
    }

    /// What Endora is pursuing.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    /// The belief this came from — never absent, so "why is it doing this?" always has
    /// an answer that points at correctable understanding.
    #[must_use]
    pub const fn motivating_belief(&self) -> BeliefId {
        self.motivating_belief
    }

    /// Where it got to last time, in the butler's own words. Empty before the first
    /// night's work.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// Its current state.
    #[must_use]
    pub const fn state(&self) -> IntentionState {
        self.state
    }

    /// When Endora took it up.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When it last moved.
    #[must_use]
    pub const fn last_progressed_at(&self) -> Timestamp {
        self.last_progressed_at
    }

    /// How many nights have gone into it.
    #[must_use]
    pub const fn steps_taken(&self) -> u32 {
        self.steps_taken
    }
}

#[cfg(test)]
mod tests {
    use super::{Intention, IntentionState, STEP_BUDGET};
    use endora_kernel::ids::{BeliefId, IntentionId, Timestamp};

    const DAY: i64 = 24 * 60 * 60 * 1_000;

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    fn formed() -> Intention {
        Intention::form(
            IntentionId::new(1),
            "learn what helps them sleep",
            BeliefId::new(7),
            at(0),
        )
        .expect("a named intention is valid")
    }

    #[test]
    fn an_intention_always_names_the_belief_it_came_from() {
        // ADR 0036: no free-floating intentions, so "why is it doing this?" always has
        // an answer. Enforced by the type — this test documents the intent of that.
        assert_eq!(formed().motivating_belief(), BeliefId::new(7));
    }

    #[test]
    fn a_nameless_intention_is_rejected() {
        assert!(
            Intention::form(IntentionId::new(1), "   ", BeliefId::new(7), at(0)).is_err(),
            "an intention that names nothing can't be pursued or explained"
        );
    }

    #[test]
    fn it_starts_active_with_nothing_learned_yet() {
        let intention = formed();
        assert!(intention.is_active());
        assert_eq!(intention.note(), "");
        assert_eq!(intention.steps_taken(), 0);
    }

    #[test]
    fn progress_keeps_the_butlers_own_words_for_next_time() {
        // The resumption mechanism (ADR 0036): prose in, prose out, no state machine
        // for the model to maintain.
        let mut intention = formed();
        intention.progress("  They mentioned the room being too warm.  ", at(DAY));
        assert_eq!(intention.note(), "They mentioned the room being too warm.");
        assert_eq!(intention.steps_taken(), 1);
        assert_eq!(intention.last_progressed_at(), at(DAY));
    }

    #[test]
    fn it_gives_up_after_the_step_budget() {
        let mut intention = formed();
        for night in 1..=STEP_BUDGET {
            assert!(
                !intention.is_exhausted(),
                "not spent after {night} night(s) yet"
            );
            intention.progress("looked into it", at(i64::from(night) * DAY));
        }
        assert!(intention.is_exhausted());

        let why = intention.retire_if_over(at(i64::from(STEP_BUDGET) * DAY));
        assert_eq!(why, Some("gave it seven nights without getting anywhere"));
        assert!(!intention.is_active());
        assert_eq!(intention.state(), IntentionState::Abandoned);
    }

    #[test]
    fn it_goes_stale_when_nothing_happens_for_a_fortnight() {
        // What happens when the nightly loop is off — it must not sit there forever
        // looking like something Endora is doing.
        let mut intention = formed();
        assert!(!intention.is_stale(at(13 * DAY)));
        assert!(intention.is_stale(at(14 * DAY)));

        let why = intention.retire_if_over(at(14 * DAY));
        assert_eq!(why, Some("hadn't made progress on it in a fortnight"));
        assert!(!intention.is_active());
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_age_anything() {
        assert!(!formed().is_stale(at(-100 * DAY)));
    }

    #[test]
    fn retiring_leaves_a_finished_intention_alone() {
        // Only an *active* one retires; a completed one keeps its state and its story.
        let mut intention = formed();
        intention.complete();
        assert_eq!(intention.retire_if_over(at(999 * DAY)), None);
        assert_eq!(intention.state(), IntentionState::Done);
    }

    #[test]
    fn the_person_can_drop_it() {
        // Their only verb over an intention (ADR 0036).
        let mut intention = formed();
        intention.abandon();
        assert!(!intention.is_active());
        assert_eq!(intention.state(), IntentionState::Abandoned);
    }

    #[test]
    fn an_unreadable_state_reads_as_abandoned_not_as_active() {
        // A corrupt row must never resurrect itself into something Endora keeps
        // working on overnight.
        assert_eq!(
            IntentionState::from_name("bogus"),
            IntentionState::Abandoned
        );
        for state in [
            IntentionState::Active,
            IntentionState::Done,
            IntentionState::Abandoned,
        ] {
            assert_eq!(IntentionState::from_name(state.name()), state);
        }
    }
}
