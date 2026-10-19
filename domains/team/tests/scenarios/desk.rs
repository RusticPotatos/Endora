//! The steps of `features/a-desk-that-interrupts-only-for-decisions.feature` (ADR 0132): what
//! interrupts the owner and what waits for them in the digest, as the HQ screen's snapshot serves
//! them, and the one finding kept for a task and a guard.

use std::sync::atomic::Ordering;

use cucumber::{then, when};
use endora_team::application::engine::STRANDED_AFTER_MS;
use endora_team::application::guard_ports::GuardStore as _;
use endora_team::application::{Author, TeamStore};
use endora_team::domain::TaskStatus;
use endora_team::domain::guards::Verdict;
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

    /// The name of the member who holds the task, the way the owner reads it.
    fn holder_of(&self, title: &str) -> String {
        let task = self
            .hq
            .store()
            .task(self.task(title))
            .expect("the store answers")
            .expect("a task");
        let holder = task
            .owner
            .or(task.assignee)
            .unwrap_or_else(|| panic!("nobody holds {title:?}"));
        self.members
            .iter()
            .find(|(_, id)| **id == holder)
            .map(|(name, _)| name.clone())
            .expect("a member of the team")
    }

    /// The task's stuck finding, the newest one.
    fn stuck_finding_on(&self, title: &str) -> endora_team::domain::guards::Finding {
        let key = format!("stuck:{}", self.task(title).0);
        self.hq
            .store()
            .latest_finding(&key)
            .expect("the store answers")
            .unwrap_or_else(|| panic!("{title:?} has no stuck finding"))
    }

    /// Time passes with nobody back on the work: a nudge, its turn over, and ten minutes, twice,
    /// and once more.
    fn left_alone(&mut self) {
        for _ in 0..2 {
            self.clock
                .0
                .fetch_add(STRANDED_AFTER_MS + 1, Ordering::SeqCst);
            self.tick();
            for started in self.hq.runner().started() {
                self.hq.runner().finish(started.run, true);
            }
            self.tick();
        }
        self.clock
            .0
            .fetch_add(STRANDED_AFTER_MS + 1, Ordering::SeqCst);
        self.tick();
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

#[then(regex = r#"^the digest holds 1 item: stuck work on "(.+)"$"#)]
fn the_digest_holds_stuck_work(w: &mut Office, title: String) {
    let waiting = w.in_the_digest();
    assert_eq!(waiting.len(), 1, "in the digest: {waiting:?}");
    assert_eq!(waiting[0]["kind"], "stuck", "{waiting:?}");
    assert_eq!(waiting[0]["task"], json!(w.task(&title).0), "{waiting:?}");
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

/// The holder takes a task it was handed and blocks it, saying on what; then, for that and for
/// work already blocked or taken, time passes with nobody to take it up, until HQ puts it in the
/// digest as stuck.
#[when(expr = "{string} is left stuck with no lead to take it")]
fn left_stuck(w: &mut Office, title: String) {
    let name = w.holder_of(&title);
    let task = w.task(&title);
    let status =
        w.hq.store()
            .task(task)
            .expect("the store answers")
            .expect("a task")
            .status;
    if status == TaskStatus::Todo {
        w.use_tool(&name, "claim", &json!({ "task": task.0 }))
            .expect("the member takes its task");
        w.use_tool(
            &name,
            "set_status",
            &json!({ "task": task.0, "status": "blocked",
                     "reason": "the indexer is down until its update lands tonight" }),
        )
        .expect("the member blocks its task");
        super::turn_ends(w, name);
    }
    w.left_alone();
}

#[then(regex = r#"^there (?:is|are) (\d+) stuck findings? on "(.+)"$"#)]
fn stuck_findings(w: &mut Office, count: usize, title: String) {
    let key = format!("stuck:{}", w.task(&title).0);
    let found =
        w.hq.store()
            .findings()
            .expect("the store answers")
            .into_iter()
            .filter(|f| f.key == key)
            .count();
    assert_eq!(found, count, "the findings on {title:?}");
}

#[then(expr = "the stuck finding on {string} was last seen again over an hour after it was found")]
fn seen_again_an_hour_later(w: &mut Office, title: String) {
    let finding = w.stuck_finding_on(&title);
    assert!(
        finding.seen_at - finding.opened_at > 60 * 60 * 1000,
        "seen at {}, found at {}",
        finding.seen_at,
        finding.opened_at
    );
    assert_eq!(finding.episodes, 1, "the same episode");
}

/// The owner writes in the thread and the holder's turn on it ends having done nothing.
#[when(regex = r#"^(\w+) looks at "(.+)" and leaves it where it was$"#)]
fn looks_and_leaves_it(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.hq.post(Author::Owner, task, "Try the indexer again.")
        .expect("the owner may write in a thread");
    w.turn_of(&name);
    super::turn_ends(w, name);
}

/// The owner writes, and the holder is back on the work and moves it on.
#[when(regex = r#"^(\w+) moves "(.+)" on$"#)]
fn moves_it_on(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.hq.post(Author::Owner, task, "Try the indexer again.")
        .expect("the owner may write in a thread");
    w.use_tool(
        &name,
        "set_status",
        &json!({ "task": task.0, "status": "doing", "reason": "the indexer is back" }),
    )
    .expect("the member moves it on");
    super::turn_ends(w, name);
}

#[when(expr = "the owner judges the stuck finding on {string} resolved")]
fn owner_judges_stuck_finding(w: &mut Office, title: String) {
    let finding = w.stuck_finding_on(&title);
    w.hq.judge_finding(finding.id, Verdict::Resolved, None)
        .expect("the owner judges it");
}

#[when("the owner says the finding is resolved")]
fn owner_says_resolved(w: &mut Office) {
    let item = w
        .waiting_on_desk("guard")
        .pop()
        .expect("a finding on the desk");
    w.hq.decide(item.id, "resolved").expect("the owner decides");
}

#[then(regex = r#"^there (?:is|are) (\d+) findings? of "(.+)"$"#)]
fn findings_of(w: &mut Office, count: usize, guard: String) {
    let found =
        w.hq.store()
            .findings()
            .expect("the store answers")
            .into_iter()
            .filter(|f| f.guard.plainly() == guard)
            .count();
    assert_eq!(found, count, "the findings of {guard:?}");
}
