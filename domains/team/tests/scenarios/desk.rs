//! The steps of `features/a-desk-that-interrupts-only-for-decisions.feature` (ADR 0132): what
//! interrupts the owner and what waits for them in the digest, as the HQ screen's snapshot serves
//! them.

use cucumber::{then, when};
use serde_json::{Value, json};

use super::Office;

impl Office {
    /// What interrupts the owner, as the screen's desk lists it.
    fn on_the_desk(&self) -> Vec<Value> {
        self.hq.snapshot().expect("the snapshot is built")["desk"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// What waits for the owner in the digest, newest first.
    fn in_the_digest(&self) -> Vec<Value> {
        self.hq.snapshot().expect("the snapshot is built")["digest"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

// --- What interrupts, and what waits -----------------------------------------------------------

#[then(regex = r"^the owner is interrupted by (\d+) items?$")]
fn interrupted_by(w: &mut Office, count: usize) {
    let on_the_desk = w.on_the_desk();
    assert_eq!(on_the_desk.len(), count, "on the desk: {on_the_desk:?}");
}

#[then("the digest is empty")]
fn the_digest_is_empty(w: &mut Office) {
    let waiting = w.in_the_digest();
    assert!(waiting.is_empty(), "in the digest: {waiting:?}");
}

#[then(regex = r#"^the digest holds 1 item: a (finding|notice) "(.+)"$"#)]
fn the_digest_holds_one(w: &mut Office, what: String, starting: String) {
    let waiting = w.in_the_digest();
    assert_eq!(waiting.len(), 1, "in the digest: {waiting:?}");
    let kind = if what == "finding" { "guard" } else { "notice" };
    assert_eq!(waiting[0]["kind"], kind, "{waiting:?}");
    let text = waiting[0]["text"].as_str().unwrap_or_default();
    assert!(text.starts_with(&starting), "the card says: {text}");
}

#[when(expr = "Endora tells the owner {string}")]
fn endora_tells_the_owner(w: &mut Office, text: String) {
    w.hq.notice(None, &text).expect("a notice is put up");
}

#[when("the owner marks it seen")]
fn the_owner_marks_it_seen(w: &mut Office) {
    let waiting = w.in_the_digest();
    let item = waiting.first().expect("something in the digest");
    let id = item["id"].as_i64().expect("an item has an id");
    w.hq.decide(endora_team::domain::DeskId(id), "seen")
        .expect("the owner decides");
}

#[then(
    regex = r"^the outcomes count (\d+) interruptions? and (\d+) items? in the digest this week$"
)]
fn the_desk_is_counted(w: &mut Office, interrupts: u64, digest: u64) {
    let week =
        w.hq.snapshot().expect("the snapshot is built")["scorecard"]["outcomes"]["week"].clone();
    assert_eq!(week["interrupts"], json!(interrupts), "{week}");
    assert_eq!(week["digest"], json!(digest), "{week}");
}

// --- One finding for a task and a guard --------------------------------------------------------
