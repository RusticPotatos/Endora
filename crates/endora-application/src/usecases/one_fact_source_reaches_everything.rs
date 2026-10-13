//! The seam that makes adding an integration free (ADR 0059).
//!
//! A source used to reach a turn only by implementing `about_the_person` and phrasing
//! itself — which exactly one integration ever did. Everything else was reachable in
//! principle and silent in practice, waiting for a weak model to think of calling it. The
//! morning brief taught that lesson once and it was never generalised.
//!
//! Now: say what you currently know, and you are watched, your changes reach the turn, and
//! your records feed the thinking. No nomination, no wiring per source.

use super::{MOST_CHANGES_IN_A_TURN, what_changed_lately};
use endora_capabilities::{Transition, TransitionLog, Watched};
use endora_kernel::RepositoryError;

struct Moved(Vec<Transition>);
impl TransitionLog for Moved {
    fn watching(&self) -> Result<Vec<Watched>, RepositoryError> {
        Ok(Vec::new())
    }
    fn remember(&self, _w: &Watched) -> Result<(), RepositoryError> {
        Ok(())
    }
    fn record(&self, _t: &Transition) -> Result<(), RepositoryError> {
        Ok(())
    }
    fn since(&self, ms: i64) -> Result<Vec<Transition>, RepositoryError> {
        Ok(self.0.iter().filter(|t| t.at_ms >= ms).cloned().collect())
    }
    fn forget_before(&self, _ms: i64) -> Result<(), RepositoryError> {
        Ok(())
    }
}

fn moved(key: &str, at_ms: i64) -> Transition {
    Transition {
        key: key.to_owned(),
        from: "closed".to_owned(),
        to: "open".to_owned(),
        at_ms,
    }
}

const NOW: i64 = 10_000_000_000;

#[test]
fn a_source_that_only_says_what_it_knows_reaches_the_turn() {
    // No `about_the_person`, no nomination, no wiring — it changed, so the turn hears
    // about it. That is the whole of the contract.
    let log = Moved(vec![moved("nextdoor::posts", NOW - 60_000)]);
    let said = what_changed_lately(&log, NOW);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("nextdoor::posts"), "{said:?}");
    assert!(
        said[0].contains("closed"),
        "it should say what it was: {said:?}"
    );
    assert!(said[0].contains("open"), "and what it is: {said:?}");
}

#[test]
fn a_quiet_source_says_nothing() {
    // The turn is not a log. A source that has not moved contributes nothing at all,
    // which is what keeps this affordable on a weak model.
    assert!(what_changed_lately(&Moved(Vec::new()), NOW).is_empty());
}

#[test]
fn yesterdays_news_is_not_news() {
    let log = Moved(vec![moved("house::door", NOW - 48 * 60 * 60 * 1_000)]);
    assert!(what_changed_lately(&log, NOW).is_empty());
}

#[test]
fn a_busy_house_does_not_swamp_the_turn() {
    // The budget is the reason this can be automatic instead of opt-in. Forty changes in
    // front of every answer would have made the context worse, not better — and then it
    // would have had to be something the person nominated per source.
    let log = Moved((0..40).map(|i| moved(&format!("k{i}"), NOW - i)).collect());
    assert_eq!(what_changed_lately(&log, NOW).len(), MOST_CHANGES_IN_A_TURN);
}

#[test]
fn the_newest_changes_are_the_ones_that_survive_the_cap() {
    let log = Moved(vec![
        moved("old", NOW - 5 * 60 * 60 * 1_000),
        moved("recent", NOW - 60_000),
    ]);
    let said = what_changed_lately(&log, NOW);
    assert!(said[0].contains("recent"), "newest first: {said:?}");
}

#[test]
fn a_log_that_will_not_answer_costs_the_turn_nothing() {
    // Context assembly must never fail a turn. A source that cannot be read is a fact
    // Endora does not have, which is the same as any other fact it does not have.
    struct Broken;
    impl TransitionLog for Broken {
        fn watching(&self) -> Result<Vec<Watched>, RepositoryError> {
            Err(RepositoryError::Backend("no".to_owned()))
        }
        fn remember(&self, _w: &Watched) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn record(&self, _t: &Transition) -> Result<(), RepositoryError> {
            Ok(())
        }
        fn since(&self, _ms: i64) -> Result<Vec<Transition>, RepositoryError> {
            Err(RepositoryError::Backend("no".to_owned()))
        }
        fn forget_before(&self, _ms: i64) -> Result<(), RepositoryError> {
            Ok(())
        }
    }
    assert!(what_changed_lately(&Broken, NOW).is_empty());
}
