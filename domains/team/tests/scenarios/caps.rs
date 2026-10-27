//! The steps of `features/caps-on-talk-and-runaway.feature` (ADR 0133): a day's cap on the
//! house's own models, and the runaway tripwire.

use std::sync::atomic::Ordering;

use endora_team::application::run_ports::RunStore;

use cucumber::{then, when};
use endora_team::application::TeamStore;
use endora_team::domain::{RunStatus, WakeReason};
use serde_json::json;

use super::Office;

#[when(regex = r#"^(\w+) is asked to talk about "(.+)" again$"#)]
fn asked_to_talk(w: &mut Office, name: String, title: String) {
    let member = w.member(&name);
    let task = w.task(&title);
    w.hq.wake(member, Some(task), WakeReason::Mentioned)
        .expect("a wake has a receipt");
}

impl Office {}

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
}
