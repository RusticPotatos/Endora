//! The steps of `features/accepted-work-reaches-the-fork.feature` (ADR 0115, amended
//! 2026-10-31): accepted work is listed on Manage, pushed by the node once its privacy scan is
//! clean, merged into develop behind a setting, and the cards come back behind another. The
//! node is played here as it calls HQ: it asks what is due, and says what came of it.

use cucumber::{given, then, when};

use endora_team::application::desk_ports::DeskStore;
use endora_team::application::wing_ports::WingStore;
use endora_team::application::{Delivered, Found};

use endora_team::domain::DeskId;

use super::Office;

/// The newest desk item of `kind`, whatever its status.
fn newest_card(w: &Office, kind: &str) -> DeskId {
    w.hq.store()
        .desk()
        .expect("the store answers")
        .into_iter()
        .filter(|d| d.kind == kind)
        .map(|d| d.id)
        .max_by_key(|id| id.0)
        .unwrap_or_else(|| panic!("no {kind} card"))
}

impl Office {
    /// The branch HQ recorded under `name`.
    fn branch_named(&self, name: &str) -> endora_team::domain::Branch {
        self.hq
            .store()
            .branches()
            .expect("the store answers")
            .into_iter()
            .filter(|b| b.name == name)
            .max_by_key(|b| b.id)
            .unwrap_or_else(|| panic!("no branch {name}"))
    }

    /// The row Manage's "Accepted, waiting to merge" shows for `branch`, if any.
    fn accepted_row(&self, branch: &str) -> Option<serde_json::Value> {
        let snap = self.hq.snapshot().expect("the snapshot is built");
        snap["accepted"]
            .as_array()
            .expect("the snapshot lists accepted work")
            .iter()
            .find(|r| r["branch"] == branch)
            .cloned()
    }
}

#[then(expr = "the owner's desk holds no {string} card")]
fn no_card_of(w: &mut Office, kind: String) {
    let items = w.waiting_on_desk(&kind);
    assert!(items.is_empty(), "{items:?}");
}

#[then(expr = "the owner's desk holds one {string} card saying {string}")]
fn one_card_saying(w: &mut Office, kind: String, text: String) {
    let items = w.waiting_on_desk(&kind);
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].text.contains(&text), "{}", items[0].text);
}

#[then(expr = "the owner's desk holds nothing about {string}")]
fn nothing_about(w: &mut Office, title: String) {
    let task = w.task(&title);
    let about: Vec<_> = w
        .waiting_on_desk("")
        .into_iter()
        .filter(|d| d.task == Some(task))
        .collect();
    assert!(about.is_empty(), "{about:?}");
}

#[then(expr = "Manage lists {string} for {string} as accepted, waiting to merge")]
fn manage_lists(w: &mut Office, branch: String, title: String) {
    let row = w
        .accepted_row(&branch)
        .unwrap_or_else(|| panic!("{branch} is not listed"));
    assert_eq!(row["title"], title.as_str(), "{row}");
    assert!(row["at"].as_i64().is_some(), "when: {row}");
}

#[then(expr = "Manage shows {string} as on the fork, waiting to merge")]
fn manage_on_fork(w: &mut Office, branch: String) {
    let row = w.accepted_row(&branch).expect("listed");
    assert_eq!(row["status"], "on_fork", "{row}");
}

#[then(expr = "Manage shows {string} as held by the privacy scan")]
fn manage_held(w: &mut Office, branch: String) {
    let row = w.accepted_row(&branch).expect("listed");
    assert_eq!(row["status"], "held", "{row}");
}

#[then(expr = "Manage no longer lists {string}")]
fn manage_drops(w: &mut Office, branch: String) {
    assert!(w.accepted_row(&branch).is_none());
}

#[then(expr = "the node is asked to push {string}")]
fn asked_to_push(w: &mut Office, branch: String) {
    let due = w.hq.deliveries().expect("the store answers");
    let found = due.push.iter().find(|d| d.branch.name == branch);
    assert!(found.is_some_and(|d| d.scan), "{due:?}");
}

#[then(expr = "the node is asked to push {string} without its scan")]
fn asked_to_push_unscanned(w: &mut Office, branch: String) {
    let due = w.hq.deliveries().expect("the store answers");
    let found = due.push.iter().find(|d| d.branch.name == branch);
    assert!(found.is_some_and(|d| !d.scan), "{due:?}");
}

#[then(expr = "the node is not asked to push {string}")]
fn not_asked_to_push(w: &mut Office, branch: String) {
    let due = w.hq.deliveries().expect("the store answers");
    assert!(due.push.iter().all(|d| d.branch.name != branch), "{due:?}");
}

#[then(expr = "the node is asked to merge {string}")]
fn asked_to_merge(w: &mut Office, branch: String) {
    let due = w.hq.deliveries().expect("the store answers");
    assert!(due.merge.iter().any(|b| b.name == branch), "{due:?}");
}

#[then(expr = "the node is not asked to merge {string}")]
fn not_asked_to_merge(w: &mut Office, branch: String) {
    let due = w.hq.deliveries().expect("the store answers");
    assert!(due.merge.iter().all(|b| b.name != branch), "{due:?}");
}

#[when(expr = "the node pushes {string}, its privacy scan clean")]
fn node_pushes(w: &mut Office, branch: String) {
    let id = w.branch_named(&branch).id;
    w.hq.delivered(
        id,
        &Delivered::Pushed {
            ok: true,
            text: format!("pushed {branch} to the fork"),
            needs: Vec::new(),
        },
    )
    .expect("the node's word is taken");
}

#[when(expr = "the node pushes {string}, finding it needs {string} at {string}")]
fn node_pushes_needing(w: &mut Office, branch: String, what: String, at: String) {
    let id = w.branch_named(&branch).id;
    w.hq.delivered(
        id,
        &Delivered::Pushed {
            ok: true,
            text: format!("pushed {branch} to the fork"),
            needs: vec![Found { what, at }],
        },
    )
    .expect("the node's word is taken");
}

#[when(expr = "the node's privacy scan of {string} finds {string} at {string}")]
fn scan_finds(w: &mut Office, branch: String, what: String, at: String) {
    let id = w.branch_named(&branch).id;
    w.hq.delivered(
        id,
        &Delivered::Held {
            found: vec![Found { what, at }],
        },
    )
    .expect("the node's word is taken");
}

#[when(expr = "the node merges {string} into develop")]
fn node_merges(w: &mut Office, branch: String) {
    let id = w.branch_named(&branch).id;
    w.hq.delivered(
        id,
        &Delivered::Merged {
            ok: true,
            text: format!("merged {branch} into develop"),
        },
    )
    .expect("the node's word is taken");
}

#[when("the owner sends it back")]
fn owner_sends_back(w: &mut Office) {
    let card = newest_card(w, "privacy");
    w.hq.decide(card, "send_back").expect("the owner decides");
}

#[when("the owner pushes it anyway")]
fn owner_pushes_anyway(w: &mut Office) {
    let card = newest_card(w, "privacy");
    w.hq.decide(card, "push_anyway").expect("the owner decides");
}

#[when("the owner marks it merged")]
fn owner_marks_merged(w: &mut Office) {
    let card = newest_card(w, "ready_to_merge");
    w.hq.decide(card, "merged").expect("the owner decides");
}

#[then("merging develop is off")]
fn merging_is_off(w: &mut Office) {
    let snap = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(
        snap["delivery"]["merge_develop"], false,
        "{}",
        snap["delivery"]
    );
}

#[given("the owner turns merging develop on")]
fn merging_on(w: &mut Office) {
    w.hq.set_delivery(None, Some(true))
        .expect("the owner's setting is kept");
}

#[given("the owner puts the merge cards back on the desk")]
fn cards_back(w: &mut Office) {
    w.hq.set_delivery(Some(true), None)
        .expect("the owner's setting is kept");
}

#[when("the owner takes the merge cards off the desk again")]
fn cards_off(w: &mut Office) {
    w.hq.set_delivery(Some(false), None)
        .expect("the owner's setting is kept");
}

#[then(expr = "Manage shows {string} as kept, its item blocked")]
fn manage_kept_blocked(w: &mut Office, branch: String) {
    let row = w.accepted_row(&branch).expect("listed");
    assert_eq!(row["status"], "kept_blocked", "{row}");
}
