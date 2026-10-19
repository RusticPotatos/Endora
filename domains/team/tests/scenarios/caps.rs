//! The steps of `features/caps-on-talk-and-runaway.feature` (ADR 0133): a task's budget of talk,
//! tried in shadow before the owner switches it on.

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
use endora_team::domain::guards::{Guard, GuardMode};
use endora_team::domain::{Receipt, Refused, WakeReason};

use super::Office;

#[given("the owner switches the coordination guard on")]
#[when("the owner switches the coordination guard on")]
fn switches_on(w: &mut Office) {
    w.hq.set_guard_mode(Guard::Coordination, GuardMode::On)
        .expect("the owner may switch the guard on");
}

#[when("the owner puts the coordination guard back in shadow")]
fn back_in_shadow(w: &mut Office) {
    w.hq.set_guard_mode(Guard::Coordination, GuardMode::Shadow)
        .expect("the owner may put the guard back in shadow");
}

#[then("the coordination guard is in shadow")]
fn is_in_shadow(w: &mut Office) {
    assert_eq!(
        w.hq.guard_mode(Guard::Coordination)
            .expect("the store answers"),
        GuardMode::Shadow
    );
    // And the owner's Guards card says so.
    let shown = w.hq.snapshot().expect("the snapshot is built")["guards"].clone();
    let row = shown["precision"]
        .as_array()
        .expect("each guard's precision")
        .iter()
        .find(|p| p["guard"] == "coordination")
        .cloned()
        .expect("the guard is on the card");
    assert_eq!(row["mode"], "shadow", "{row}");
}

/// Another ask for talk: a mention in the task's thread, as a member's post makes.
#[when(regex = r#"^(\w+) is asked to talk about "(.+)" again$"#)]
fn asked_to_talk(w: &mut Office, name: String, title: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    w.hq.wake(member, Some(task), WakeReason::Mentioned)
        .expect("a wake has a receipt");
}

impl Office {
    /// The newest wake for `name`: what became of the last thing that asked for its turn. (The
    /// wake that came after it may be the lead's, woken to decide.)
    fn newest_wake_of(&self, name: &str) -> endora_team::application::ports::Wake {
        let member = self.member(name);
        self.hq
            .wakes()
            .expect("the store answers")
            .into_iter()
            .rfind(|wake| wake.member == member)
            .unwrap_or_else(|| panic!("{name} was never woken"))
    }
}

#[then(regex = r"^(\w+)'s wake is held$")]
fn the_wake_is_held(w: &mut Office, name: String) {
    let wake = w.newest_wake_of(&name);
    assert_eq!(wake.receipt, Receipt::Held, "{wake:?}");
}

#[then(regex = r"^(\w+)'s wake is queued, not held$")]
fn the_wake_is_queued(w: &mut Office, name: String) {
    let wake = w.newest_wake_of(&name);
    assert_eq!(wake.receipt, Receipt::Queued, "{wake:?}");
}

#[then(regex = r"^(\w+)'s wake is a duplicate of the one held$")]
fn the_wake_is_a_duplicate(w: &mut Office, name: String) {
    let wake = w.newest_wake_of(&name);
    assert_eq!(
        wake.receipt,
        Receipt::Refused(Refused::Duplicate),
        "{wake:?}"
    );
}

impl Office {
    /// How many times HQ told the task's thread it is over its budget of talk.
    fn budget_posts(&self, title: &str) -> usize {
        let task = self.task(title);
        self.hq
            .store()
            .thread(task)
            .expect("the store answers")
            .iter()
            .filter(|p| p.author == "endora" && p.text.contains("over its budget"))
            .count()
    }
}

/// One decision, put to the lead of the task's area: said in the thread, and the lead woken.
#[then(regex = r#"^(\w+) is asked once to narrow, split or drop "(.+)"$"#)]
fn asked_once(w: &mut Office, name: String, title: String) {
    assert_eq!(w.budget_posts(&title), 1, "HQ asks in the thread, once");
    let member = w.member(&name);
    let task = w.task(&title);
    let asked =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| {
                wake.member == member && wake.task == Some(task) && wake.reason == WakeReason::Guard
            })
            .count();
    assert_eq!(asked, 1, "and wakes {name} to decide it, once");
    let said =
        w.hq.store()
            .thread(task)
            .expect("the store answers")
            .into_iter()
            .rfind(|p| p.text.contains("over its budget"))
            .expect("HQ asked");
    for way in ["narrow_criteria", "create_task", "decide_stuck"] {
        assert!(said.text.contains(way), "{way}: {}", said.text);
    }
}

#[then(regex = r#"^(\w+) was not asked to decide anything about "(.+)"$"#)]
fn not_asked(w: &mut Office, _name: String, title: String) {
    assert_eq!(w.budget_posts(&title), 0);
}
