//! The steps of `features/reviews-are-capped-at-haiku.feature` and
//! `features/a-ci-run-hq-cannot-get.feature` (ADR 0142, amended 2026-10-31): who reviews an
//! item, and what HQ's checks say of a criterion that asks for a CI run.

use cucumber::then;

use endora_team::domain::{Receipt, WakeReason};

use super::Office;

impl Office {
    /// Whether `name` was woken to review the item titled `title`.
    fn woken_to_review(&self, name: &str, title: &str) -> bool {
        let (member, task) = (self.member(name), self.task(title));
        self.hq
            .wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|w| !matches!(w.receipt, Receipt::Refused(_)))
            .any(|w| {
                w.member == member
                    && w.task == Some(task)
                    && w.reason == WakeReason::ReviewRequested
            })
    }
}

#[then(expr = "{word} is woken to review {string}")]
fn woken_to_review(w: &mut Office, name: String, title: String) {
    assert!(
        w.woken_to_review(&name, &title),
        "{name} was not woken to review {title:?}"
    );
}

#[then(expr = "{word} is not woken to review {string}")]
fn not_woken_to_review(w: &mut Office, name: String, title: String) {
    assert!(
        !w.woken_to_review(&name, &title),
        "{name} was woken to review {title:?}"
    );
}
