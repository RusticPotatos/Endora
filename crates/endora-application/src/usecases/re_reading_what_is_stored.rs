use super::*;
use endora_kernel::Timestamp;
use std::sync::Mutex;

#[derive(Default)]
struct Remembered(Mutex<Vec<Belief>>);

impl BeliefRepository for Remembered {
    fn save(&self, belief: &Belief) -> Result<(), endora_kernel::RepositoryError> {
        let mut held = self.0.lock().unwrap();
        if let Some(slot) = held.iter_mut().find(|b| b.id() == belief.id()) {
            *slot = belief.clone();
        } else {
            held.push(belief.clone());
        }
        Ok(())
    }
    fn get(&self, id: BeliefId) -> Result<Option<Belief>, endora_kernel::RepositoryError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|b| b.id() == id)
            .cloned())
    }
    fn list(&self) -> Result<Vec<Belief>, endora_kernel::RepositoryError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

fn stored(id: u128, statement: &str, created_ms: i64) -> Belief {
    Belief::from_parts(
        BeliefId::new(id),
        statement.to_owned(),
        crate::BeliefKind::Preference,
        crate::Confidence::High,
        "because you said so".to_owned(),
        Timestamp::from_unix_millis(created_ms),
        Timestamp::from_unix_millis(created_ms),
        crate::BeliefStatus::Active,
    )
}

fn still_held(repo: &Remembered) -> Vec<String> {
    repo.list()
        .unwrap()
        .into_iter()
        .filter(|b| b.status() == crate::BeliefStatus::Active)
        .map(|b| b.statement().to_owned())
        .collect()
}

#[test]
fn a_belief_the_rules_would_no_longer_form_does_not_get_to_stay() {
    // The card that prompted this: stored before the rule against instruction-shaped
    // statements existed, and untouchable afterwards because guards only ever ran at
    // formation. Rules improving has to reach backwards or the store is frozen at the
    // quality of the day each row was written.
    let repo = Remembered::default();
    repo.save(&stored(
        1,
        "you want me to turn off the kitchen light",
        1_000,
    ))
    .unwrap();
    repo.save(&stored(2, "you run on Tuesdays and Thursdays", 2_000))
        .unwrap();

    let retired = tidy_understanding(&repo, &FixedClock(9_000)).unwrap();

    assert_eq!(retired, 1);
    assert_eq!(still_held(&repo), vec!["you run on Tuesdays and Thursdays"]);
    // Expired, not corrected: the person never said it was wrong, and saying they did
    // would put words in their mouth.
    let dropped = repo.get(BeliefId::new(1)).unwrap().unwrap();
    assert_eq!(dropped.status(), crate::BeliefStatus::Expired);
}

#[test]
fn three_ways_of_saying_one_thing_become_one_thing_the_oldest() {
    // Exactly what was on screen: three Fahrenheit cards, each asking separately.
    let repo = Remembered::default();
    repo.save(&stored(1, "you prefer temperatures in Fahrenheit", 1_000))
        .unwrap();
    repo.save(&stored(
        2,
        "You prefer temperature measurements in Fahrenheit.",
        2_000,
    ))
    .unwrap();
    repo.save(&stored(
        3,
        "You find it more convenient and accurate to measure temperature in Fahrenheit",
        3_000,
    ))
    .unwrap();

    let retired = tidy_understanding(&repo, &FixedClock(9_000)).unwrap();

    assert_eq!(retired, 2);
    assert_eq!(
        still_held(&repo),
        vec!["you prefer temperatures in Fahrenheit"]
    );
    // The survivor is the oldest, and the duplicates count as evidence for it rather
    // than being thrown away — the same thought arriving three times is why it is
    // worth being sure about.
    let survivor = repo.get(BeliefId::new(1)).unwrap().unwrap();
    assert!(survivor.last_affirmed_at().unix_millis() > survivor.created_at().unix_millis());
}

#[test]
fn two_beliefs_that_disagree_are_both_left_alone() {
    // Merging these would silently pick a winner. Endora holding both is the honest
    // state, and which one is true is the person's call (ADR 0052).
    let repo = Remembered::default();
    repo.save(&stored(1, "you like running", 1_000)).unwrap();
    repo.save(&stored(2, "you hate running", 2_000)).unwrap();

    assert_eq!(tidy_understanding(&repo, &FixedClock(9_000)).unwrap(), 0);
    assert_eq!(still_held(&repo).len(), 2);
}

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(self.0)
    }
}
