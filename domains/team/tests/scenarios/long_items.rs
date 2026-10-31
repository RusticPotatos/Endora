//! The steps of `features/a-long-item-makes-progress-across-turns.feature` (ADR 0142, amended
//! 2026-10-31, D12): a turn reads the newest handover, not every one; the work a turn leaves is
//! kept for the next; and a turn out of room that kept new work is no failure.

use cucumber::{then, when};
use serde_json::json;

use endora_team::application::wing_ports::WingStore;
use endora_team::application::{Submission, TeamStore};
use endora_team::domain::handover::START;

use super::Office;

/// What the worker posts as a turn's last word when its prompt reached its budget, about as
/// long as round 2's were (450 to 950 characters).
const OUT_OF_ROOM: &str = "Now write the rule, its fixtures and its tests.\n\nThe turn's prompt \
     reached its budget (447,000 bytes read over 8 calls, of 480000): it ends here, and goes on \
     in a fresh turn from HQ's handover. This turn did: checked out the fork; ran `cd /work/repo \
     && git checkout -q -b hq/task-937-secret-defau…`; ran `cd /work/repo && sed -n 40,194p \
     tests/lint_rules/nas_rules.p…`; ran `cd /work/repo && cut -c1-140 tests/test_lint_rules.py; \
     cat t…`; ran `cd /work/repo && python3 - <<'EOF' import re,glob K=re.compi…`.";

impl Office {
    /// One turn of `name`'s on `title` that ends as the worker ends a turn whose prompt reached
    /// its budget, near a Claude turn's 120,000 tokens: keeping its work on `kept` (a slug,
    /// under the item's own `hq/task-N-`) first, when it has some.
    fn out_of_room(&mut self, name: &str, title: &str, kept: Option<&str>) {
        let task = self.task(title);
        let turn = self.turn_of(name);
        assert_eq!(turn.task, Some(task), "{name}'s turn is on {title:?}");
        self.use_tool(name, "claim", &json!({ "task": task.0 }));
        if let Some(slug) = kept {
            let kept_before = self.hq.store().branches().expect("the store answers").len();
            self.hq
                .keep_work_in_progress(
                    &turn.token,
                    &Submission {
                        branch: format!("hq/task-{}-{slug}", task.0),
                        head: format!("{:040x}", kept_before + 1),
                        base: "ee878f7619e9".to_owned(),
                        files: 3,
                        plus: 40,
                        minus: 0,
                        stat: " tests/lint_rules/nas_rules.py | 40 ++++".to_owned(),
                        diff: "+class SecretDefault:".to_owned(),
                        can_push: false,
                        changed: vec!["tests/lint_rules/nas_rules.py".to_owned()],
                        texts: Vec::new(),
                        workflows: None,
                    },
                )
                .expect("the work in progress is kept");
        }
        self.runs_out_of_room(name, 100_000, OUT_OF_ROOM);
    }

    /// What `name` reads first on `title`: its turn's `my_turn`, the briefing and the thread.
    fn reads_of(&mut self, name: &str, title: &str) -> String {
        let task = self.task(title);
        let turn = self.turn_of(name);
        assert_eq!(turn.task, Some(task), "{name}'s turn is on {title:?}");
        self.use_tool(name, "my_turn", &json!({}))
            .expect("my_turn answers")
    }
}

#[when(regex = r#"^(\w+)'s turns on "(.+)" run out of room (\d+) times, keeping nothing$"#)]
fn out_of_room_keeping_nothing(w: &mut Office, name: String, title: String, times: usize) {
    for _ in 0..times {
        w.out_of_room(&name, &title, None);
    }
}

#[when(regex = r#"^(\w+)'s turns on "(.+)" run out of room (\d+) times, each keeping new work$"#)]
fn out_of_room_keeping_work(w: &mut Office, name: String, title: String, times: usize) {
    for _ in 0..times {
        w.out_of_room(&name, &title, Some("secret-defaults"));
    }
}

#[when(regex = r#"^(\w+)'s turn on "(.+)" runs out of room, keeping its work on "(.+)"$"#)]
fn out_of_room_keeping(w: &mut Office, name: String, title: String, slug: String) {
    w.out_of_room(&name, &title, Some(&slug));
}

#[then(regex = r#"^what (\w+) reads of "(.+)" holds (\d+) handovers?$"#)]
fn reads_handovers(w: &mut Office, name: String, title: String, count: usize) {
    let read = w.reads_of(&name, &title);
    assert_eq!(read.matches(START).count(), count, "{read}");
}

#[then(regex = r#"^what (\w+) reads of "(.+)" says older handovers were left out$"#)]
fn says_left_out(w: &mut Office, name: String, title: String) {
    let read = w.reads_of(&name, &title);
    assert!(read.contains("older handovers"), "{read}");
    assert!(
        !read.contains("This turn did:"),
        "the last words before the newest handover are read only as it carries them: {read}"
    );
}

#[then(expr = "the thread of {string} still holds every handover")]
fn thread_keeps_every_handover(w: &mut Office, title: String) {
    let thread =
        w.hq.store()
            .thread(w.task(&title))
            .expect("the store answers");
    let handovers = thread.iter().filter(|p| p.text.starts_with(START)).count();
    let out_of_room = thread
        .iter()
        .filter(|p| p.author != "endora" && p.text.contains("The turn's prompt reached its budget"))
        .count();
    assert!(out_of_room > 1, "{thread:?}");
    assert_eq!(
        handovers, out_of_room,
        "one handover after each: {thread:?}"
    );
}

#[then(expr = "no check is queued for {string}")]
fn no_check_queued(w: &mut Office, title: String) {
    let task = w.task(&title);
    let checks: Vec<_> =
        w.hq.store()
            .checks()
            .expect("the store answers")
            .into_iter()
            .filter(|c| c.task == task)
            .collect();
    assert!(checks.is_empty(), "{checks:?}");
}

#[then(regex = r#"^what (\w+) reads of "(.+)" says to check out its kept branch "(.+)"$"#)]
fn says_check_out(w: &mut Office, name: String, title: String, slug: String) {
    let task = w.task(&title);
    let read = w.reads_of(&name, &title);
    let branch = format!("hq/task-{}-{slug}", task.0);
    assert!(
        read.contains(&format!("repo_checkout with branch {branch}")),
        "{read}"
    );
}

#[then(regex = r#"^"(.+)" has (\d+) failures? and is still (\w+)'s$"#)]
fn failures_still_held(w: &mut Office, title: String, failures: u32, name: String) {
    assert_eq!(w.failures(&title), failures);
    assert_eq!(w.holder_of(&title), Some(name));
}
