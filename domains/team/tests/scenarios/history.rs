//! The steps of `features/the-history.feature` (ADR 0129): what every turn said, kept for a
//! while and read back in the turn's replay, and what stays when it goes.

use cucumber::{then, when};
use endora_team::application::{Clock as _, TeamStore};
use endora_team::domain::history::{DAY_MS, KEPT_DAYS, TRANSCRIPT_MOST};
use serde_json::json;

use super::Office;

impl Office {
    /// The turn `name` last had, as the owner's replay reads it.
    fn replay_of(&self, name: &str) -> serde_json::Value {
        let run = self
            .turns
            .get(name)
            .unwrap_or_else(|| panic!("{name}'s turn has not started"))
            .run;
        self.hq.replay_run(run).expect("the replay is read").shown()
    }
}

#[when(expr = "{word}'s worker tells the node what the turn said: {string}")]
fn worker_tells(w: &mut Office, name: String, said: String) {
    let token = w.turn_of(&name).token;
    let kept =
        w.hq.record_transcript(&token, "worker", &format!("--- assistant ---\n{said}"));
    assert!(w.note(kept).is_some(), "refused: {:?}", w.refused);
}

#[when(
    expr = "{word}'s worker tells the node a conversation of two megabytes that starts {string} and ends {string}"
)]
fn worker_tells_a_lot(w: &mut Office, name: String, start: String, end: String) {
    let token = w.turn_of(&name).token;
    let text = format!("{start}{}{end}", "a line of talk\n".repeat(150_000));
    let kept = w.hq.record_transcript(&token, "worker", &text);
    assert!(w.note(kept).is_some(), "refused: {:?}", w.refused);
}

#[when(expr = "{word} says {string} as the last word")]
fn says_last_word(w: &mut Office, name: String, text: String) {
    let task = w.turn_of(&name).task.expect("a turn on a task");
    let posted = w.use_tool(
        &name,
        "post",
        &json!({ "task": task.0, "text": text, "last_word": true }),
    );
    assert!(posted.is_some(), "refused: {:?}", w.refused);
}

#[then(expr = "the replay of {word}'s turn shows {string}")]
fn replay_shows(w: &mut Office, name: String, text: String) {
    let shown = w.replay_of(&name);
    let said = shown["transcript"]["text"].as_str().unwrap_or_default();
    assert!(said.contains(&text), "the replay holds: {shown}");
}

#[then(expr = "the replay of {word}'s turn shows no conversation")]
fn replay_shows_none(w: &mut Office, name: String) {
    let shown = w.replay_of(&name);
    assert!(shown["transcript"].is_null(), "the replay holds: {shown}");
}

#[then(regex = r"^the replay of (\w+)'s turn says it is kept for (\d+) days$")]
fn replay_says_kept_for(w: &mut Office, name: String, days: i64) {
    assert_eq!(days, KEPT_DAYS, "the owner's rule is thirty days");
    let shown = w.replay_of(&name);
    let until = shown["transcript"]["kept_until"].as_i64().expect("a date");
    assert_eq!(until, w.clock.now() + days * DAY_MS, "{shown}");
}

#[then(
    expr = "the replay of {word}'s turn is cut, and still starts with {string} and ends with {string}"
)]
fn replay_is_cut(w: &mut Office, name: String, start: String, end: String) {
    let shown = w.replay_of(&name);
    let said = shown["transcript"]["text"].as_str().unwrap_or_default();
    assert_eq!(shown["transcript"]["cut"], true, "{shown}");
    assert!(said.len() <= TRANSCRIPT_MOST);
    assert!(said.starts_with(&start) && said.ends_with(&end));
}

#[then(expr = "the handover note on {string} still says {string}")]
fn handover_still_says(w: &mut Office, title: String, text: String) {
    use endora_team::application::world_ports::WorldStore as _;
    let task = w.task(&title);
    let notes = w.hq.store().handovers(task).expect("the store answers");
    assert!(
        notes.iter().any(|h| h.note.next.contains(&text)),
        "{notes:?}"
    );
}

#[then(expr = "{string} is still in the thread of {string}")]
fn still_in_thread(w: &mut Office, text: String, title: String) {
    let task = w.task(&title);
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(thread.iter().any(|p| p.text.contains(&text)), "{thread:?}");
}
