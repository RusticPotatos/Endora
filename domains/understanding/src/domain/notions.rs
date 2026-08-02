//! Notions — what Endora is still thinking about (ADR 0057).
//!
//! A [`Belief`](super::Belief) is something Endora holds about the person. A [`Notion`] is the
//! same statement *before* there is enough evidence to hold it: a thing it suspects, the
//! records that suggested it, and what would settle the question.
//!
//! Between beliefs and outcomes there was nowhere to put an unfinished thought, so every turn
//! began from zero and Endora could only ever report. This is the place a thought can survive
//! a night.
//!
//! Three rules from the record are enforced here rather than promised anywhere:
//!
//! - **Evidence is counted distinctly.** The same record cited three times is one piece of
//!   evidence, not three. Nothing matures by being repeated.
//! - **Re-citing what is already cited does not count as support**, so a notion cannot be kept
//!   alive by restating its own past. Silence kills it.
//! - **Maturity is arithmetic**, never the model's judgement (ADR 0051).
//!
//! Whether a citation points at anything real is checked *outside* the domain, where the
//! records live — a `Notion` can only be built from citations that already resolved.

use endora_kernel::error::{DomainError, require_non_empty};
use endora_kernel::ids::{NotionId, Timestamp};

/// One day in milliseconds.
const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

/// How many notions Endora may hold at once.
///
/// **A constant, deliberately, and not a setting** (ADR 0057). A bound the person can raise
/// from a screen is not a bound, and the failure mode of every store like this is speculation
/// accumulating faster than it resolves. Small enough that displacing the weakest is a real
/// cost, which is what keeps a new notion from being free.
pub const MOST_NOTIONS_AT_ONCE: usize = 5;

/// Distinct pieces of evidence before a notion is worth believing.
///
/// Two is a coincidence. This is the whole of "maturity" — there is no model judgement in it,
/// because the one thing this design cannot afford is a language model deciding when its own
/// speculation has been confirmed.
pub const ENOUGH_TO_BELIEVE: usize = 3;

/// How long a notion may go unsupported before it dies on its own.
///
/// The honest reading of an old open notion is not "still under consideration" but "nothing
/// has supported this in a fortnight", and keeping it would rebuild the queue ADR 0029
/// deleted. Shorter than any belief half-life: a notion has earned far less patience than
/// something Endora actually believes.
pub const UNSUPPORTED_FOR_MS: i64 = 14 * DAY_MS;

/// What kind of record a citation points at.
///
/// Deliberately a closed set: every variant is something Endora can go and fetch. A citation
/// that cannot be resolved is not weak evidence, it is a discarded notion, and that check
/// needs a finite list of places to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Something Endora did, and what changed.
    Outcome,
    /// Something the person said.
    Message,
    /// Something Endora already believes.
    Belief,
    /// A reading taken from a capability — an entity and its state.
    ///
    /// **Shared.** The house has other people in it, so a light, a door or a family calendar
    /// says nothing about any particular one of them.
    Reading,
    /// A reading from something that belongs to **the person themselves** — their phone,
    /// their watch, the tracker their own presence is computed from.
    ///
    /// Distinct from [`Reading`](Self::Reading) because the difference is the whole of
    /// attribution: a hallway light is the household's, and a watch on their wrist is not.
    /// Which entities these are is **stated by the service**, never inferred here — a wrong
    /// guess does not produce a wrong reading, it produces a wrong belief about somebody.
    Personal,
}

impl Source {
    /// Stable, lowercase name for storage and the protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Outcome => "outcome",
            Self::Message => "message",
            Self::Belief => "belief",
            Self::Reading => "reading",
            Self::Personal => "personal",
        }
    }

    /// Parses from a [`name`](Self::name).
    ///
    /// Unlike the soft parses elsewhere in understanding, this one **fails** on an unknown
    /// label rather than falling back. Everywhere else a mystery value is a cosmetic loss; here
    /// it would be a citation nobody knows how to check, which is precisely the thing that must
    /// never be stored.
    pub fn from_name(name: &str) -> Result<Self, DomainError> {
        match name {
            "outcome" => Ok(Self::Outcome),
            "message" => Ok(Self::Message),
            "belief" => Ok(Self::Belief),
            "reading" => Ok(Self::Reading),
            "personal" => Ok(Self::Personal),
            _ => Err(DomainError::EmptyField {
                field: "notion.citation.source",
            }),
        }
    }
}

/// A pointer to one record that suggested a notion.
///
/// The `reference` is whatever identifies the record in its own store — a row id, an entity
/// id. It is compared as text, so two citations are the same evidence when they name the same
/// record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Citation {
    source: Source,
    reference: String,
}

impl Citation {
    /// Names one record.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `reference` is blank — an unnamed record cannot be
    /// checked, and an uncheckable citation is the failure this whole type exists to prevent.
    pub fn new(source: Source, reference: &str) -> Result<Self, DomainError> {
        let reference = require_non_empty("notion.citation.reference", reference)?;
        Ok(Self { source, reference })
    }

    /// Which store to look in.
    #[must_use]
    pub const fn source(&self) -> Source {
        self.source
    }

    /// Which record.
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }
}

/// Where a notion is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotionStatus {
    /// Still being thought about.
    Open,
    /// Earned its confidence and became a belief.
    Matured,
    /// Nothing supported it, or it was displaced.
    Died,
}

impl NotionStatus {
    /// Stable name for storage/protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Matured => "matured",
            Self::Died => "died",
        }
    }

    /// Parses from a name; unknown becomes `Died`.
    ///
    /// The cautious direction: an unreadable notion should stop being thought about, not
    /// quietly rejoin the working set.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "open" => Self::Open,
            "matured" => Self::Matured,
            _ => Self::Died,
        }
    }
}

/// One thing Endora suspects but has not earned the right to believe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notion {
    id: NotionId,
    statement: String,
    citations: Vec<Citation>,
    settles_when: String,
    created_at: Timestamp,
    last_supported_at: Timestamp,
    status: NotionStatus,
}

impl Notion {
    /// Forms a new, open notion.
    ///
    /// # Errors
    /// - [`DomainError::EmptyField`] if `statement` is blank.
    /// - [`DomainError::EmptyField`] if there are no citations. **A notion with no evidence is
    ///   not a tentative thought, it is a sentence a language model produced** — the single
    ///   most important rejection in this type.
    pub fn new(
        id: NotionId,
        statement: &str,
        citations: Vec<Citation>,
        settles_when: &str,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let statement = require_non_empty("notion.statement", statement)?;
        if citations.is_empty() {
            return Err(DomainError::EmptyField {
                field: "notion.citations",
            });
        }
        let mut citations = citations;
        citations.sort();
        citations.dedup();
        Ok(Self {
            id,
            statement,
            citations,
            settles_when: settles_when.trim().to_owned(),
            created_at: at,
            last_supported_at: at,
            status: NotionStatus::Open,
        })
    }

    /// Reconstitutes a stored notion (all fields explicit).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        id: NotionId,
        statement: String,
        citations: Vec<Citation>,
        settles_when: String,
        created_at: Timestamp,
        last_supported_at: Timestamp,
        status: NotionStatus,
    ) -> Self {
        Self {
            id,
            statement,
            citations,
            settles_when,
            created_at,
            last_supported_at,
            status,
        }
    }

    /// Records a new piece of evidence, returning whether it was actually new.
    ///
    /// **Re-citing something already cited changes nothing at all** — not the evidence count,
    /// and not the clock. That second part is what makes [`has_gone_quiet`](Self::has_gone_quiet)
    /// mean anything: without it, a notion could keep itself alive forever by restating its own
    /// past, and a nightly pass re-reading the same week of records would do exactly that
    /// without anybody intending it.
    pub fn support(&mut self, citation: Citation, at: Timestamp) -> bool {
        if self.citations.contains(&citation) {
            return false;
        }
        self.citations.push(citation);
        self.citations.sort();
        self.last_supported_at = at;
        true
    }

    /// How much distinct evidence stands behind it.
    #[must_use]
    pub fn support_count(&self) -> usize {
        self.citations.len()
    }

    /// Whether there is now enough evidence for this to become a belief.
    ///
    /// Arithmetic over resolved records, and nothing else (ADR 0051): the model proposes the
    /// wording, the record decides whether it survives.
    ///
    /// **Enough is not only a count.** A belief is a statement about *the person*, so at least
    /// one piece of evidence has to be attributable to them — see
    /// [`says_something_about_the_person`](Self::says_something_about_the_person).
    #[must_use]
    pub fn is_ready_to_believe(&self) -> bool {
        self.status == NotionStatus::Open
            && self.support_count() >= ENOUGH_TO_BELIEVE
            && self.says_something_about_the_person()
    }

    /// Whether anything behind this notion is actually attributable to the person.
    ///
    /// **The house is shared.** Other people live there, so a reading — a light, a door, a
    /// panel disarmed at six in the morning, an entry on a family calendar naming two other
    /// people — says nothing about *this* person. A belief formed from one is Endora
    /// confidently wrong about them on the strength of somebody else's morning, and wrong
    /// invisibly, because the card reads perfectly well.
    ///
    /// An **outcome is Endora's own conduct**, which [`crate::domain::Belief`] already may not
    /// be formed from: a butler that treats its own behaviour as evidence reinforces whatever
    /// it happens to be doing, including its own faults. Letting one carry a notion would be
    /// that same loop with an extra step in it.
    ///
    /// So a notion may be *corroborated* by the house and by what Endora did, and may only be
    /// **carried into belief** by something the person said, or by something already believed
    /// about them.
    ///
    /// There is a real cost, taken deliberately: a pattern purely in household rhythm can
    /// never mature. That is the honest outcome while nothing records *who* did it — and it is
    /// also what keeps Endora from quietly accumulating a model of people who are not its
    /// person and never asked to be modelled.
    #[must_use]
    pub fn says_something_about_the_person(&self) -> bool {
        self.citations
            .iter()
            .any(|c| matches!(c.source, Source::Message | Source::Belief | Source::Personal))
    }

    /// Promotes it to a belief, if it has earned that. Returns whether it did.
    pub fn mature(&mut self) -> bool {
        if !self.is_ready_to_believe() {
            return false;
        }
        self.status = NotionStatus::Matured;
        true
    }

    /// Whether nothing has supported this for long enough that holding it is dishonest.
    #[must_use]
    pub fn has_gone_quiet(&self, now: Timestamp) -> bool {
        self.status == NotionStatus::Open
            && now.unix_millis() - self.last_supported_at.unix_millis() >= UNSUPPORTED_FOR_MS
    }

    /// Lets it go — nothing supported it, or something better took its place.
    pub fn die(&mut self) {
        self.status = NotionStatus::Died;
    }

    /// How this notion ranks when the cap forces one out: **least evidence first, then
    /// longest silent**.
    ///
    /// Evidence leads because it is the thing that means something. Recency only breaks ties,
    /// and only in the direction that favours the notion something spoke to more recently.
    #[must_use]
    pub fn standing(&self) -> (usize, i64) {
        (self.support_count(), self.last_supported_at.unix_millis())
    }

    /// Its identifier.
    #[must_use]
    pub const fn id(&self) -> NotionId {
        self.id
    }
    /// What Endora suspects.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
    /// The records that suggested it.
    #[must_use]
    pub fn citations(&self) -> &[Citation] {
        &self.citations
    }
    /// What would settle the question.
    #[must_use]
    pub fn settles_when(&self) -> &str {
        &self.settles_when
    }
    /// When it was first formed.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
    /// When something last supported it.
    #[must_use]
    pub const fn last_supported_at(&self) -> Timestamp {
        self.last_supported_at
    }
    /// Where it is in its life.
    #[must_use]
    pub const fn status(&self) -> NotionStatus {
        self.status
    }
}

/// Which open notion should make way for a new one, once the cap is reached.
///
/// Returns `None` while there is room. The caller kills the returned notion and stores the new
/// one — displacement is *how* the cap holds, so there is no path that grows the set.
#[must_use]
pub fn make_way_for_a_new_one(open: &[Notion]) -> Option<NotionId> {
    if open.len() < MOST_NOTIONS_AT_ONCE {
        return None;
    }
    open.iter().min_by_key(|n| n.standing()).map(Notion::id)
}

#[cfg(test)]
mod tests {
    use super::{
        Citation, ENOUGH_TO_BELIEVE, MOST_NOTIONS_AT_ONCE, Notion, NotionStatus, Source,
        UNSUPPORTED_FOR_MS, make_way_for_a_new_one,
    };
    use endora_kernel::error::DomainError;
    use endora_kernel::ids::{NotionId, Timestamp};

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms)
    }

    fn cite(reference: &str) -> Citation {
        Citation::new(Source::Outcome, reference).unwrap()
    }

    fn notion_from(references: &[&str]) -> Notion {
        Notion::new(
            NotionId::new(1),
            "the Monday gym block gets cancelled",
            references.iter().map(|r| cite(r)).collect(),
            "whether next Monday's block survives",
            at(0),
        )
        .unwrap()
    }

    #[test]
    fn forms_and_keeps_fields() {
        let n = notion_from(&["outcome-7"]);
        assert_eq!(n.statement(), "the Monday gym block gets cancelled");
        assert_eq!(n.settles_when(), "whether next Monday's block survives");
        assert_eq!(n.status(), NotionStatus::Open);
        assert_eq!(n.support_count(), 1);
    }

    #[test]
    fn a_notion_with_no_evidence_is_not_a_notion() {
        // The single most important rejection here: without this, a notion is just a sentence
        // the model produced, and the whole design becomes a fabrication store (ADR 0057).
        assert_eq!(
            Notion::new(NotionId::new(1), "you seem tired lately", vec![], "", at(0)),
            Err(DomainError::EmptyField {
                field: "notion.citations"
            })
        );
    }

    #[test]
    fn rejects_a_blank_statement() {
        assert_eq!(
            Notion::new(NotionId::new(1), "  ", vec![cite("outcome-1")], "", at(0)),
            Err(DomainError::EmptyField {
                field: "notion.statement"
            })
        );
    }

    #[test]
    fn an_unnamed_record_cannot_be_cited() {
        assert_eq!(
            Citation::new(Source::Message, "   "),
            Err(DomainError::EmptyField {
                field: "notion.citation.reference"
            })
        );
    }

    #[test]
    fn the_same_record_cited_twice_is_one_piece_of_evidence() {
        // Repetition is the cheapest way for a model to manufacture confidence, at formation
        // and afterwards. Both doors are shut.
        let mut n = notion_from(&["outcome-7", "outcome-7", "outcome-9"]);
        assert_eq!(n.support_count(), 2, "duplicates collapse at formation");
        assert!(!n.support(cite("outcome-9"), at(50)), "already cited");
        assert_eq!(n.support_count(), 2);
    }

    #[test]
    fn restating_what_it_already_knows_does_not_keep_it_alive() {
        // The guarantee that makes death-by-silence real. A nightly pass re-reading the same
        // week of records would otherwise renew every notion forever without anyone meaning
        // it, and nothing would ever expire.
        let mut n = notion_from(&["outcome-7"]);
        let much_later = at(UNSUPPORTED_FOR_MS);
        assert!(!n.support(cite("outcome-7"), much_later));
        assert_eq!(
            n.last_supported_at(),
            at(0),
            "an old record re-cited must not touch the clock"
        );
        assert!(n.has_gone_quiet(much_later));
    }

    #[test]
    fn new_evidence_resets_the_clock() {
        let mut n = notion_from(&["outcome-7"]);
        let late = at(UNSUPPORTED_FOR_MS - 1);
        assert!(n.support(cite("outcome-8"), late));
        assert!(!n.has_gone_quiet(at(UNSUPPORTED_FOR_MS)));
        assert!(n.has_gone_quiet(at(late.unix_millis() + UNSUPPORTED_FOR_MS)));
    }

    #[test]
    fn it_matures_on_a_count_and_never_on_request() {
        // ADR 0057 rejects letting the model decide it was right. Maturity is arithmetic, so
        // asking early simply fails.
        let mut n = notion_from(&["outcome-1"]);
        // Something the person said, so attribution is satisfied and the count is the only
        // thing this test is still about.
        assert!(n.support(Citation::new(Source::Message, "msg-4").unwrap(), at(5)));
        assert_eq!(n.support_count(), 2);
        assert!(!n.is_ready_to_believe());
        assert!(!n.mature(), "two is a coincidence");
        assert_eq!(n.status(), NotionStatus::Open);

        assert!(n.support(cite("outcome-3"), at(10)));
        assert_eq!(n.support_count(), ENOUGH_TO_BELIEVE);
        assert!(n.mature());
        assert_eq!(n.status(), NotionStatus::Matured);
    }

    #[test]
    fn evidence_may_come_from_different_kinds_of_record() {
        // A calendar reading, something the person said, and something Endora did are three
        // distinct pieces of evidence — which is what makes a cross-source pattern possible.
        let mut n = notion_from(&["outcome-1"]);
        assert!(n.support(Citation::new(Source::Message, "msg-2").unwrap(), at(1)));
        assert!(n.support(Citation::new(Source::Reading, "msg-2").unwrap(), at(2)));
        assert_eq!(
            n.support_count(),
            3,
            "same reference in different stores is not the same record"
        );
    }

    #[test]
    fn the_house_alone_never_becomes_a_belief_about_the_person() {
        // Other people live there. A reading is a light, a door, a panel disarmed at six in
        // the morning, an entry on a family calendar naming two other people — none of which
        // says anything about *this* person. Three of them is still nothing about them.
        let mut n = Notion::new(
            NotionId::new(1),
            "you are up early lately",
            vec![
                Citation::new(Source::Reading, "alarm_control_panel.home").unwrap(),
                Citation::new(Source::Reading, "binary_sensor.front_door").unwrap(),
                Citation::new(Source::Reading, "calendar.family").unwrap(),
            ],
            "",
            at(0),
        )
        .unwrap();
        assert_eq!(n.support_count(), ENOUGH_TO_BELIEVE);
        assert!(!n.is_ready_to_believe(), "the house is shared");
        assert!(!n.mature());
        assert_eq!(
            n.status(),
            NotionStatus::Open,
            "still worth wondering about"
        );
    }

    #[test]
    fn what_endora_did_cannot_carry_a_notion_either() {
        // An outcome is Endora's own conduct, and a butler that treats its own behaviour as
        // evidence reinforces whatever it happens to be doing — the loop ADR 0052 found at the
        // top of the understanding screen, arriving here with an extra step in it.
        let mut n = notion_from(&["outcome-1", "outcome-2", "outcome-3"]);
        assert_eq!(n.support_count(), ENOUGH_TO_BELIEVE);
        assert!(!n.is_ready_to_believe());
        assert!(!n.mature());
    }

    #[test]
    fn a_reading_from_the_persons_own_device_can_carry_a_notion() {
        // The house is shared; a watch on their wrist is not. Ruling out every reading was
        // right about the hallway light and wrong about their own phone, which is the only
        // source besides conversation that says anything about *them* on its own.
        let mut n = Notion::new(
            NotionId::new(1),
            "you are up early lately",
            vec![
                Citation::new(Source::Personal, "device_tracker.rustic_phone").unwrap(),
                Citation::new(Source::Reading, "alarm_control_panel.home").unwrap(),
                Citation::new(Source::Outcome, "outcome-9").unwrap(),
            ],
            "",
            at(0),
        )
        .unwrap();
        assert!(n.is_ready_to_believe());
        assert!(n.mature());
    }

    #[test]
    fn one_thing_the_person_said_is_enough_to_carry_it() {
        // The house may corroborate; it may not carry. So a notion the person spoke to even
        // once, met twice more by the house, is a belief about them.
        let mut n = Notion::new(
            NotionId::new(1),
            "you are up early lately",
            vec![Citation::new(Source::Message, "msg-4").unwrap()],
            "",
            at(0),
        )
        .unwrap();
        n.support(
            Citation::new(Source::Reading, "alarm_control_panel.home").unwrap(),
            at(10),
        );
        n.support(Citation::new(Source::Outcome, "outcome-9").unwrap(), at(20));
        assert!(n.is_ready_to_believe());
        assert!(n.mature());
    }

    #[test]
    fn a_matured_notion_stops_being_thought_about() {
        let mut n = notion_from(&["a", "b"]);
        n.support(Citation::new(Source::Message, "msg-4").unwrap(), at(1));
        assert!(n.mature());
        assert!(!n.is_ready_to_believe(), "no longer open");
        assert!(
            !n.has_gone_quiet(at(999 * UNSUPPORTED_FOR_MS)),
            "it became a belief; it did not fade away"
        );
    }

    #[test]
    fn a_dead_notion_is_never_reported_as_quiet() {
        // The same double-counting guard beliefs have: something already gone must not also
        // appear in the expiry trail.
        let mut n = notion_from(&["a"]);
        n.die();
        assert!(!n.has_gone_quiet(at(999 * UNSUPPORTED_FOR_MS)));
    }

    #[test]
    fn a_clock_that_goes_backwards_kills_nothing() {
        let n = Notion::new(
            NotionId::new(1),
            "you cook more at weekends",
            vec![cite("outcome-1")],
            "",
            at(10 * UNSUPPORTED_FOR_MS),
        )
        .unwrap();
        assert!(!n.has_gone_quiet(at(0)));
    }

    // --- The cap (ADR 0057) ---

    #[test]
    fn there_is_room_until_there_is_not() {
        let open: Vec<Notion> = (0..MOST_NOTIONS_AT_ONCE - 1)
            .map(|i| {
                Notion::new(
                    NotionId::new(i as u128),
                    "something",
                    vec![cite("outcome-1")],
                    "",
                    at(0),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(make_way_for_a_new_one(&open), None);
    }

    #[test]
    fn the_least_supported_notion_makes_way() {
        let mut open: Vec<Notion> = (0..MOST_NOTIONS_AT_ONCE)
            .map(|i| {
                let mut n = Notion::new(
                    NotionId::new(i as u128),
                    "something",
                    vec![cite("shared")],
                    "",
                    at(0),
                )
                .unwrap();
                // Everyone but #3 has picked up a second piece of evidence.
                if i != 3 {
                    n.support(cite(&format!("extra-{i}")), at(100));
                }
                n
            })
            .collect();
        assert_eq!(make_way_for_a_new_one(&open), Some(NotionId::new(3)));

        // Once it catches up, the tie is broken by which has been silent longest.
        open[3].support(cite("extra-3"), at(500));
        assert_eq!(make_way_for_a_new_one(&open), Some(NotionId::new(0)));
    }

    #[test]
    fn status_and_source_names_round_trip() {
        for s in [
            NotionStatus::Open,
            NotionStatus::Matured,
            NotionStatus::Died,
        ] {
            assert_eq!(NotionStatus::from_name(s.name()), s);
        }
        assert_eq!(NotionStatus::from_name("bogus"), NotionStatus::Died);
        for s in [
            Source::Outcome,
            Source::Message,
            Source::Belief,
            Source::Reading,
            Source::Personal,
        ] {
            assert_eq!(Source::from_name(s.name()).unwrap(), s);
        }
    }

    #[test]
    fn an_unknown_citation_source_is_refused_rather_than_guessed() {
        // Everywhere else in understanding an unknown label softens to a default. Not here: a
        // citation nobody knows how to check is exactly what must never be stored.
        assert!(Source::from_name("vibes").is_err());
    }
}
