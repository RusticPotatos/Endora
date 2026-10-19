//! The steps of `features/the-two-brains.feature` (ADR 0131): a member works with the Mac or the
//! house's model and judges with a Claude, and HQ decides from why a turn was asked for which of
//! them it runs on.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use endora_team::application::{Author, Engine, NewTask, TeamStore};
use endora_team::domain::money::{DAY_MS, DOLLAR, LEAST_A_TURN};
use endora_team::domain::{Brain, Priority, Receipt, Refused, Role, RunStatus, TaskId, WakeReason};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

use super::Office;

impl Office {
    /// An HQ with the settings this scenario has now, over its file if it keeps one.
    fn restart_with_settings(&mut self) {
        if let Some(path) = self.file.clone() {
            self.hq = super::mac::hq_over(self, &path);
            return;
        }
        self.hq = Engine::new(
            SqliteTeamStore::in_memory().expect("an in-memory store"),
            SimulatedRunner::new(4),
            self.clock.clone(),
            self.settings(),
        );
    }

    /// The task with this title, made if the scenario has none yet: nobody has it, so its
    /// making wakes no one.
    fn a_task_nobody_has(&mut self, title: &str) -> TaskId {
        let known = self
            .hq
            .store()
            .tasks()
            .expect("the store answers")
            .into_iter()
            .find(|t| t.title == title);
        if let Some(task) = known {
            return task.id;
        }
        let goal = self.goal();
        self.add_task(
            Author::Owner,
            NewTask {
                goal,
                project: None,
                parent: None,
                title: title.to_owned(),
                description: String::new(),
                assignee: None,
                priority: Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .expect("the owner may make a task")
    }

    fn the_member(&self, name: &str) -> endora_team::domain::Member {
        let id = self.member(name);
        self.hq
            .store()
            .members()
            .expect("the store answers")
            .into_iter()
            .find(|m| m.id == id)
            .expect("hired")
    }

    /// The turns of `name`'s that wait, with the words they wait in.
    fn waiting_turns(&self, name: &str) -> Vec<endora_team::application::ports::Run> {
        let id = self.member(name);
        self.hq
            .store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == id && r.status == RunStatus::Queued)
            .collect()
    }
}

#[given(regex = r"^turns have kinds, with a pool of \$(\d+(?:\.\d+)?) a day$")]
fn turns_have_kinds(w: &mut Office, dollars: f64) {
    w.turn_kinds = true;
    w.pool_per_month = (dollars * DOLLAR as f64 * 30.0).round() as i64;
    w.restart_with_settings();
}

#[given(expr = "{word} judges with Claude")]
fn judges_with_claude(w: &mut Office, name: String) {
    let id = w.member(&name);
    let role = w.the_member(&name).role;
    let claude = if role == Role::Researcher {
        "claude-haiku-5-5"
    } else {
        "claude-sonnet-5-5"
    };
    w.hq.set_judgment_brain(id, claude)
        .expect("a Claude the role may judge with");
}

#[given(expr = "{word} works with the Mac and judges with no one")]
fn works_with_the_mac_and_judges_with_no_one(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.set_work_brain(id, "mac")
        .expect("the Mac may be worked with");
    w.hq.set_judgment_brain(id, "none")
        .expect("a judgment brain may be taken away");
}

#[given(expr = "{word} works with the Mac")]
fn works_with_the_mac(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.set_work_brain(id, "mac")
        .expect("the Mac may be worked with");
}

#[given(expr = "{word} works with the house's model")]
fn works_with_the_house_model(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.set_work_brain(id, "local")
        .expect("the house's model may be worked with");
}

#[when(expr = "the owner gives {word} {string} to judge with")]
fn the_owner_gives_a_judgment_brain(w: &mut Office, name: String, label: String) {
    let id = w.member(&name);
    let role = w.the_member(&name).role;
    let brain = Brain::judgment_choices(role)
        .into_iter()
        .find(|b| b.label() == label)
        .unwrap_or_else(|| panic!("{label} is not a Claude a {role:?} may judge with"));
    w.hq.set_judgment_brain(id, brain.name())
        .expect("the owner may give a judgment brain");
}

/// Wakes `name` to decide `title`, as a lead's ladder or the CEO's direction does.
#[given(expr = "{word} is woken to decide {string}")]
#[when(expr = "{word} is woken to decide {string}")]
fn woken_to_decide(w: &mut Office, name: String, title: String) {
    let id = w.member(&name);
    let task = w.a_task_nobody_has(&title);
    w.hq.wake(id, Some(task), WakeReason::Decide)
        .expect("a wake has a receipt");
}

/// The pool's day is spent on `name`'s decisions: it takes turns of judgment, each spending all
/// it was allowed, until less than the least a turn can have is left.
#[given(expr = "the pool's day is spent on {word}'s decisions")]
fn the_pools_day_is_spent(w: &mut Office, name: String) {
    let id = w.member(&name);
    let title = format!("{name}'s earlier decision");
    let task = w.a_task_nobody_has(&title);
    for _ in 0..8 {
        let shown = w.hq.snapshot().expect("the snapshot is built");
        let left = shown["pool"]["per_day"].as_i64().unwrap_or(0)
            - shown["pool"]["committed"].as_i64().unwrap_or(0);
        if left < LEAST_A_TURN {
            return;
        }
        w.hq.wake(id, Some(task), WakeReason::Decide)
            .expect("a wake has a receipt");
        let context = w.turn_of(&name);
        let reserved =
            w.hq.store()
                .run(context.run)
                .expect("the store answers")
                .expect("the turn is on record")
                .reserved;
        w.hq.report_usage(context.run, reserved)
            .expect("the ledger takes it");
        w.hq.runner().finish(context.run, true);
        w.hq.tick().expect("the tick runs");
    }
    panic!("the pool's day would not be spent");
}

#[when("a day passes")]
fn a_day_passes(w: &mut Office) {
    w.clock.0.fetch_add(DAY_MS, Ordering::SeqCst);
    w.hq.tick().expect("the tick runs");
}

#[when(regex = r"^the owner tops up the pool by \$(\d+(?:\.\d+)?)$")]
fn tops_up_the_pool(w: &mut Office, dollars: f64) {
    w.hq.top_up_pool((dollars * DOLLAR as f64).round() as i64)
        .expect("the owner may top up the pool");
    w.hq.tick().expect("the tick runs");
}

#[then(expr = "{word}'s turn on {string} is waiting for budget")]
fn turn_waits_for_budget(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let waiting = w
        .waiting_turns(&name)
        .into_iter()
        .find(|r| r.task == Some(task))
        .unwrap_or_else(|| panic!("{name} has no turn waiting on {title:?}"));
    assert!(
        waiting.end_reason.starts_with("waiting for budget"),
        "{name}'s turn says: {}",
        waiting.end_reason
    );
    // The owner's screen says the same.
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let run = shown["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .find(|r| r["id"] == waiting.id.0)
        .expect("the waiting turn is on screen");
    assert_eq!(run["why"], waiting.end_reason.as_str());
}

#[then(expr = "{word}'s turn on {string} is waiting for a judgment brain")]
fn turn_waits_for_a_judgment_brain(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let waiting = w
        .waiting_turns(&name)
        .into_iter()
        .find(|r| r.task == Some(task))
        .unwrap_or_else(|| panic!("{name} has no turn waiting on {title:?}"));
    assert!(
        waiting
            .end_reason
            .starts_with("waiting for a judgment brain"),
        "{name}'s turn says: {}",
        waiting.end_reason
    );
}

#[then(regex = r"^the budget chip counts (\d+) turns? waiting$")]
fn the_chip_counts(w: &mut Office, count: u64) {
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(shown["pool"]["waiting"], count);
}

#[then(regex = r"^(\w+) has (\d+) turns? waiting for budget$")]
fn waiting_for_budget_count(w: &mut Office, name: String, count: usize) {
    let waiting = w
        .waiting_turns(&name)
        .into_iter()
        .filter(|r| r.end_reason.starts_with("waiting for budget"))
        .count();
    assert_eq!(waiting, count, "{name}'s turns waiting for budget");
}

#[then(regex = r#"^(\w+)'s turn runs on "(.+)"$"#)]
fn turn_runs_on(w: &mut Office, name: String, brain: String) {
    let context = w
        .turns
        .get(&name)
        .unwrap_or_else(|| panic!("{name}'s turn has not started"))
        .clone();
    let run =
        w.hq.store()
            .run(context.run)
            .expect("the store answers")
            .expect("the turn is on record");
    assert_eq!(
        run.brain.map(Brain::name),
        Some(brain.as_str()),
        "{name}'s turn ran on another brain"
    );
    // What the node's relay is told when it asks whose turn a token is.
    let (_, as_the_turn) =
        w.hq.turn_of(&context.token)
            .expect("the store answers")
            .expect("the turn is running");
    assert_eq!(as_the_turn.brain.name(), brain);
}

#[then("nothing is running on the Mac")]
fn nothing_runs_on_the_mac(w: &mut Office) {
    let shown = w.hq.snapshot().expect("the snapshot is built");
    assert_eq!(shown["mac"]["running"], 0);
}

#[then("no wake was refused for budget")]
fn no_wake_refused_for_budget(w: &mut Office) {
    let refused =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .filter(|wake| wake.receipt == Receipt::Refused(Refused::OutOfBudget))
            .count();
    assert_eq!(refused, 0, "a wake was thrown away for money");
}

#[then(expr = "the decision waits for {word}'s work turn to end")]
fn the_decision_is_held(w: &mut Office, name: String) {
    let wake = w.last_wake();
    assert_eq!(wake.member, w.member(&name));
    assert_eq!(wake.reason, WakeReason::Decide);
    assert_eq!(
        wake.receipt,
        Receipt::Held,
        "the decision joined the work turn"
    );
}
