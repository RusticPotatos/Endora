//! The steps of `features/a-deploy-under-a-turn.feature` (ADR 0124, amended): a turn the
//! platform cut short is tried again as the same member, and raises nothing.

use cucumber::then;
use endora_team::application::TeamStore;
use endora_team::application::guard_ports::GuardStore as _;
use endora_team::application::ports::Run;
use endora_team::domain::runs::own_attempts;
use endora_team::domain::{RunStatus, WakeReason};

use super::Office;

impl Office {
    /// `member`'s newest turn on the task titled `title`, as the HQ screen lists it.
    fn newest_turn_of(&self, member: &str, title: &str) -> Run {
        let (id, task) = (self.member(member), self.task(title));
        self.hq
            .store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == id && r.task == Some(task))
            .max_by_key(|r| r.id)
            .unwrap_or_else(|| panic!("{member} has had no turn on {title:?}"))
    }
}

#[then(expr = "{word}'s turn on {string} waits to be tried again")]
fn waits_to_be_tried_again(w: &mut Office, name: String, title: String) {
    let turn = w.newest_turn_of(&name, &title);
    assert_eq!(
        turn.status,
        RunStatus::RetryWaiting,
        "{name}'s turn on {title:?}: {turn:?}"
    );
}

#[then(expr = "that was an interruption, and {word} has all his retries")]
fn the_first_interruption_is_free(w: &mut Office, name: String) {
    // Newest, whatever the task: the scenario has one.
    let id = w.member(&name);
    let turn =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == id)
            .max_by_key(|r| r.id)
            .expect("the member had a turn");
    assert_eq!(turn.interrupted, 1, "{turn:?}");
    assert_eq!(
        own_attempts(turn.attempts, turn.interrupted),
        0,
        "none of {name}'s starts counts against him: {turn:?}"
    );
}

#[then(expr = "that was the second interruption, and {word} has one retry fewer")]
fn only_the_first_is_free(w: &mut Office, name: String) {
    let id = w.member(&name);
    let turn =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == id)
            .max_by_key(|r| r.id)
            .expect("the member had a turn");
    assert_eq!(turn.interrupted, 2, "{turn:?}");
    assert_eq!(
        own_attempts(turn.attempts, turn.interrupted),
        1,
        "the first loss was free and the second counts: {turn:?}"
    );
}

#[then("no finding is open")]
fn no_finding_is_open(w: &mut Office) {
    let open: Vec<_> =
        w.hq.store()
            .findings()
            .expect("the store answers")
            .into_iter()
            .filter(|f| f.cleared_at.is_none())
            .collect();
    assert!(open.is_empty(), "findings are open: {open:?}");
}

#[then(expr = "{word} was not nudged about {string}")]
fn was_not_nudged(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let nudges: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.member == member && wake.task == Some(task))
            .filter(|wake| {
                matches!(
                    wake.reason,
                    WakeReason::Recovery | WakeReason::AssignmentRecovery
                )
            })
            .collect();
    assert!(nudges.is_empty(), "{name} was nudged: {nudges:?}");
}

#[then(expr = "{word} was not asked to look at {string}")]
fn was_not_asked_to_look(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let asked: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.member == member && wake.task == Some(task))
            .collect();
    assert!(asked.is_empty(), "{name} was woken on {title:?}: {asked:?}");
}
