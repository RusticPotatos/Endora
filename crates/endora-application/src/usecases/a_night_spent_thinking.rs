//! The nightly pass (ADR 0057). Three of its four steps involve no model at all, which is
//! why a notion cannot be talked into being believed.

use super::tests::FakeNotions;
use super::{
    KnownRecord, ProposedNotion, Source, TheRecord, notions_proposed_in, think_about_the_day,
};
use crate::{
    Belief, BeliefRepository, Citation, Notion, NotionId, NotionRepository, NotionStatus,
    RepositoryError, Timestamp,
};
use endora_kernel::{Clock, IdSource};
use std::cell::RefCell;

struct At(i64);
impl Clock for At {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(self.0)
    }
}

#[derive(Default)]
struct Ids(std::cell::Cell<u128>);
impl IdSource for Ids {
    fn new_id(&self) -> u128 {
        self.0.set(self.0.get() + 1);
        self.0.get()
    }
}

#[derive(Default)]
struct Beliefs(RefCell<Vec<Belief>>);
impl BeliefRepository for Beliefs {
    fn save(&self, belief: &Belief) -> Result<(), RepositoryError> {
        self.0.borrow_mut().push(belief.clone());
        Ok(())
    }
    fn get(&self, _id: crate::BeliefId) -> Result<Option<Belief>, RepositoryError> {
        Ok(None)
    }
    fn list(&self) -> Result<Vec<Belief>, RepositoryError> {
        Ok(self.0.borrow().clone())
    }
    fn forget_all(&self) -> Result<usize, RepositoryError> {
        Ok(self.0.borrow_mut().drain(..).count())
    }
}

fn gym_record() -> TheRecord {
    TheRecord::of(vec![
        KnownRecord {
            source: Source::Message,
            reference: "1".to_owned(),
            text: "skipping the gym again, too tired".to_owned(),
        },
        KnownRecord {
            source: Source::Reading,
            reference: "calendar.rustic".to_owned(),
            text: "Gym".to_owned(),
        },
        KnownRecord {
            source: Source::Outcome,
            reference: "9".to_owned(),
            text: "the gym session was skipped".to_owned(),
        },
    ])
}

fn open_notion(id: u128, statement: &str, cited: &[(Source, &str)], at_ms: i64) -> Notion {
    let citations = cited
        .iter()
        .map(|(s, r)| Citation::new(*s, r).unwrap())
        .collect();
    Notion::new(
        NotionId::new(id),
        statement,
        citations,
        "",
        Timestamp::from_unix_millis(at_ms),
    )
    .unwrap()
}

// --- Reading what the model wrote ---

#[test]
fn a_well_formed_line_is_read() {
    let read = notions_proposed_in(
        "Sure! Here you go:\nNOTION: you skip the gym when tired || message:1, \
         reading:calendar.rustic || whether next Monday survives",
    );
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].statement, "you skip the gym when tired");
    assert_eq!(read[0].citations.len(), 2);
    assert_eq!(read[0].settles_when, "whether next Monday survives");
}

#[test]
fn every_way_the_model_can_mangle_it_yields_nothing() {
    // The reason it is safe to hand this job to a weak local model: each of these ends
    // with Endora having formed no thought, never with a wrong thought stored.
    for mangled in [
        "I think you might be skipping the gym.", // no shape at all
        "NOTION: || message:1 || x",              // no statement
        "NOTION: you skip the gym",               // no citations
        "NOTION: you skip the gym || || x",       // empty citations
        "NOTION: you skip the gym || vibes:1 || x", // invented source
        "NOTION: you skip the gym || message: || x", // unnamed record
        "",
    ] {
        assert!(
            notions_proposed_in(mangled).is_empty(),
            "should have read nothing from {mangled:?}"
        );
    }
}

// --- The pass ---

#[test]
fn a_notion_the_record_keeps_meeting_becomes_a_belief() {
    // The whole point: Endora arrives at something about the person by being met
    // repeatedly by records that turned up on their own.
    let notions = FakeNotions::default();
    let beliefs = Beliefs::default();
    notions
        .save(&open_notion(
            1,
            "you skip the gym when tired",
            &[(Source::Message, "1")],
            0,
        ))
        .unwrap();

    let mut activity = Vec::new();
    think_about_the_day(
        &notions,
        &beliefs,
        &gym_record(),
        &[],
        &Ids::default(),
        &At(1_000),
        &mut activity,
    )
    .unwrap();

    let held = beliefs.list().unwrap();
    assert_eq!(held.len(), 1, "three records met it, so it is now believed");
    assert_eq!(held[0].statement(), "you skip the gym when tired");
    assert!(
        held[0].evidence().contains("message:1"),
        "the belief carries the chain that produced it: {}",
        held[0].evidence()
    );
    assert_eq!(
        notions.get(NotionId::new(1)).unwrap().unwrap().status(),
        NotionStatus::Matured
    );
}

#[test]
fn a_notion_nothing_came_of_lets_itself_go() {
    let notions = FakeNotions::default();
    notions
        .save(&open_notion(
            1,
            "you have taken up the cello",
            &[(Source::Message, "1")],
            0,
        ))
        .unwrap();

    let mut activity = Vec::new();
    think_about_the_day(
        &notions,
        &Beliefs::default(),
        &TheRecord::default(),
        &[],
        &Ids::default(),
        &At(30 * 24 * 60 * 60 * 1_000),
        &mut activity,
    )
    .unwrap();

    assert_eq!(
        notions.get(NotionId::new(1)).unwrap().unwrap().status(),
        NotionStatus::Died
    );
    assert!(notions.open().unwrap().is_empty());
    assert!(activity.iter().any(|a| a.contains("nothing came of it")));
}

#[test]
fn a_night_that_landed_on_nothing_says_which_kind_of_nothing() {
    // Every failure on this path is silent by design, which left no way to tell "the model
    // said nothing" from "the model said something the record would not bear out". The
    // first is a model that cannot hit the shape and will never form a notion; the second
    // is the guarantee doing its job. They look identical from outside, and the live pass
    // produced exactly this ambiguity on its first run.
    let notions = FakeNotions::default();
    let mut activity = Vec::new();
    think_about_the_day(
        &notions,
        &Beliefs::default(),
        &gym_record(),
        &[ProposedNotion {
            statement: "you are unhappy at work".to_owned(),
            citations: vec![(Source::Message, "1".to_owned())],
            settles_when: String::new(),
        }],
        &Ids::default(),
        &At(10),
        &mut activity,
    )
    .unwrap();
    assert!(notions.list().unwrap().is_empty());
    assert_eq!(activity.len(), 1);
    assert!(
        activity[0].contains("held up against the record"),
        "should say the record refused it: {activity:?}"
    );
}

#[test]
fn a_night_with_nothing_proposed_at_all_says_so_differently() {
    let mut activity = Vec::new();
    think_about_the_day(
        &FakeNotions::default(),
        &Beliefs::default(),
        &gym_record(),
        &[],
        &Ids::default(),
        &At(10),
        &mut activity,
    )
    .unwrap();
    assert_eq!(activity.len(), 1);
    assert!(
        activity[0].contains("nothing worth watching"),
        "a model that proposed nothing is a different fact: {activity:?}"
    );
}

#[test]
fn a_night_that_did_something_does_not_also_report_nothing() {
    let mut activity = Vec::new();
    think_about_the_day(
        &FakeNotions::default(),
        &Beliefs::default(),
        &gym_record(),
        &[ProposedNotion {
            statement: "you skip the gym when tired".to_owned(),
            citations: vec![(Source::Message, "1".to_owned())],
            settles_when: String::new(),
        }],
        &Ids::default(),
        &At(10),
        &mut activity,
    )
    .unwrap();
    assert_eq!(activity.len(), 1);
    assert!(activity[0].starts_with("Started wondering"), "{activity:?}");
}

#[test]
fn a_proposal_the_record_does_not_bear_out_stores_nothing() {
    let notions = FakeNotions::default();
    let mut activity = Vec::new();
    think_about_the_day(
        &notions,
        &Beliefs::default(),
        &gym_record(),
        &[ProposedNotion {
            statement: "you are unhappy at work".to_owned(),
            citations: vec![(Source::Message, "1".to_owned())],
            settles_when: String::new(),
        }],
        &Ids::default(),
        &At(10),
        &mut activity,
    )
    .unwrap();
    assert!(notions.list().unwrap().is_empty());
}

#[test]
fn it_does_not_start_wondering_something_it_already_wonders() {
    let notions = FakeNotions::default();
    notions
        .save(&open_notion(
            1,
            "you skip the gym when you are tired",
            &[(Source::Message, "1")],
            900,
        ))
        .unwrap();

    think_about_the_day(
        &notions,
        &Beliefs::default(),
        &gym_record(),
        &[ProposedNotion {
            // A paraphrase, which is how the same thought arrives twice (ADR 0052).
            statement: "you skip the gym when tired".to_owned(),
            citations: vec![(Source::Message, "1".to_owned())],
            settles_when: String::new(),
        }],
        &Ids::default(),
        &At(1_000),
        &mut Vec::new(),
    )
    .unwrap();

    assert!(
        notions.list().unwrap().len() <= 1,
        "the same thought should not be filed twice: {:#?}",
        notions
            .list()
            .unwrap()
            .iter()
            .map(Notion::statement)
            .collect::<Vec<_>>()
    );
}

#[test]
fn it_files_at_most_one_new_thought_a_night() {
    // A pass that files five speculations about somebody in an evening is the pile of
    // chores this design exists to avoid, whatever it calls them.
    let notions = FakeNotions::default();
    let proposals: Vec<ProposedNotion> = (0..4)
        .map(|i| ProposedNotion {
            statement: format!("the gym thing number {i}"),
            citations: vec![(Source::Reading, "calendar.rustic".to_owned())],
            settles_when: String::new(),
        })
        .collect();

    think_about_the_day(
        &notions,
        &Beliefs::default(),
        &gym_record(),
        &proposals,
        &Ids::default(),
        &At(10),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(notions.open().unwrap().len(), 1);
}
