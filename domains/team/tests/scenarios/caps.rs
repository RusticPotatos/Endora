//! The steps of `features/caps-on-talk-and-runaway.feature` (ADR 0133): a task's budget of talk,
//! tried in shadow before the owner switches it on.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::TeamStore;
use endora_team::domain::guards::{Guard, GuardMode};
use endora_team::domain::{Receipt, Refused, RunStatus, WakeReason};
use serde_json::json;

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
    /// Ends every turn that runs, as its worker would.
    fn end_running_turns(&mut self) {
        for run in self
            .hq
            .store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.status == RunStatus::Running)
        {
            self.hq.runner().finish(run.id, true);
        }
    }

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

// --- A day's turns on the house's own models ----------------------------------------------

/// `turns` turns of the member's on the task, each woken by a mention and ending in a post, with
/// the clock moved on two minutes after each: five in ten minutes, nowhere near a runaway.
#[when(regex = r#"^(\w+) takes (\d+) turns on "(.+)", two minutes apart$"#)]
fn takes_turns_two_minutes_apart(w: &mut Office, name: String, turns: usize, title: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    for i in 0..turns {
        // Whatever HQ's own looks woke in the meantime (the lead, asked to decide) comes and
        // goes: a task has one turn at a time.
        for _ in 0..3 {
            w.tick();
            w.end_running_turns();
        }
        w.hq.wake(member, Some(task), WakeReason::Mentioned)
            .expect("a wake has a receipt");
        w.use_tool(
            &name,
            "post",
            &json!({ "task": task.0, "text": format!("Still weighing how to do it ({i}).") }),
        )
        .expect("a member may post in its own turn's task");
        super::turn_ends(w, name.clone());
        w.clock.0.fetch_add(2 * 60_000, Ordering::SeqCst);
    }
}

#[then(expr = "{word}'s turn on {string} is waiting for tomorrow")]
fn turn_waits_for_tomorrow(w: &mut Office, name: String, title: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    let waiting =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .find(|r| r.member == member && r.task == Some(task) && r.status == RunStatus::Queued)
            .unwrap_or_else(|| panic!("{name} has no turn waiting on {title:?}"));
    assert_eq!(
        waiting.end_reason,
        "waiting for tomorrow: 60 turns a day is the most a member takes on the house's own models"
    );
    // The owner's roster says the same.
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let run = shown["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .find(|r| r["id"] == waiting.id.0)
        .expect("the waiting turn is on screen");
    assert_eq!(run["why"], waiting.end_reason.as_str());
}

#[then(regex = r"^the top bar counts (\d+) turns? waiting for tomorrow$")]
fn the_bar_counts_tomorrow(w: &mut Office, count: u64) {
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(shown["pool"]["capped"], count);
}

// --- A runaway ------------------------------------------------------------------------------

/// Two members are woken in turn, a turn comes and goes at once for each, every `seconds`: what a
/// crash loop looks like. It goes on until HQ stops the team, or a couple of hundred have been
/// tried.
#[when(regex = r"^(\w+) and (\w+) are woken again and again, a turn every (\d+) seconds$")]
fn woken_again_and_again(w: &mut Office, first: String, second: String, seconds: i64) {
    let task = w.task("Plan the backups");
    let members = [w.member(&first), w.member(&second)];
    for tried in 0..200 {
        if w.hq.store().stopped().expect("the store answers") {
            return;
        }
        w.hq.wake(members[tried % 2], Some(task), WakeReason::Mentioned)
            .expect("a wake has a receipt");
        w.tick();
        w.end_running_turns();
        w.tick();
        w.clock.0.fetch_add(seconds * 1_000, Ordering::SeqCst);
    }
}

#[then(expr = "HQ has stopped the team, saying {string}")]
fn hq_stopped_the_team(w: &mut Office, why: String) {
    assert!(w.hq.store().stopped().expect("the store answers"));
    let said: Vec<_> =
        w.hq.store()
            .events_after(0)
            .expect("the store answers")
            .into_iter()
            .filter(|e| e.kind == "stopped")
            .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].data["why"], why.as_str());
    assert_eq!(said[0].data["by"], "tripwire");
}

#[then(regex = r"^(\d+) turns were started, and no more$")]
fn turns_were_started(w: &mut Office, count: usize) {
    let started =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .iter()
            .filter(|r| r.started_at.is_some())
            .count();
    assert_eq!(started, count);
}

#[then(regex = r"^the owner's desk holds a tripwire card naming (\w+) and (\w+)$")]
fn the_desk_holds_a_tripwire_card(w: &mut Office, first: String, second: String) {
    let cards = w.waiting_on_desk("tripwire");
    assert_eq!(cards.len(), 1, "one card: {cards:?}");
    for name in [first, second] {
        assert!(
            cards[0].text.contains(&format!("{name} ")),
            "{name}: {}",
            cards[0].text
        );
    }
    // And it interrupts: it is on the desk the owner is told about, not in the digest.
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert!(
        shown["desk"]
            .as_array()
            .expect("the desk")
            .iter()
            .any(|d| d["kind"] == "tripwire"),
        "{}",
        shown["desk"]
    );
}

#[when("the owner resumes the team")]
fn the_owner_resumes(w: &mut Office) {
    w.hq.resume().expect("the owner may resume the team");
}

#[when("the owner pulls the kill switch")]
fn the_owner_pulls_it(w: &mut Office) {
    w.hq.stop_all("trying it")
        .expect("the owner may stop the team");
}

#[then("the team is stopped by the owner")]
fn stopped_by_the_owner(w: &mut Office) {
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(shown["stopped"], true);
    assert_eq!(shown["stopped_by"], "owner");
}
