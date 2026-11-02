//! The steps of `features/a-member-is-its-model.feature` (ADR 0131, amended by 0142): a member is its
//! model, and a turn that cannot run on it now waits.

use std::sync::atomic::Ordering;

use endora_team::application::run_ports::RunStore;

use cucumber::{given, then, when};
use endora_team::application::{Author, Engine, NewTask, TeamStore};
use endora_team::domain::guards::Stopped;
use endora_team::domain::money::{DAY_MS, DOLLAR, LEAST_A_TURN};
use endora_team::domain::{Brain, Priority, Receipt, Refused, RunStatus, TaskId, WakeReason};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

use super::Office;
use endora_team::application::desk_ports::DeskStore;
use endora_team::application::event_ports::EventStore;

impl Office {
    /// An HQ with the settings this scenario has now, over its file if it keeps one.
    pub(super) fn restart_with_settings(&mut self) {
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

    /// The turns of `name`'s that wait.
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

#[given(regex = r"^a pool of \$(\d+(?:\.\d+)?) a day$")]
fn a_pool_of(w: &mut Office, dollars: f64) {
    w.pool_per_month = (dollars * DOLLAR as f64 * 30.0).round() as i64;
    w.restart_with_settings();
}

/// A member the owner hires on the house's model.
#[given(expr = "{word} is hired as a {word} on the house's model")]
fn hired_on_the_house_model(w: &mut Office, name: String, _title: String) {
    let hired = w.hq.hire_by_owner(&name, "local");
    if let Some(id) = w.note(hired) {
        w.members.insert(name, id);
    }
}

/// The owner sets `name`'s model (ADR 0142), by its label.
#[given(expr = "{word} thinks with {string}")]
#[when(expr = "the owner gives {word} {string} to think with")]
fn thinks_with(w: &mut Office, name: String, label: String) {
    let id = w.member(&name);
    let brain = Brain::all()
        .into_iter()
        .find(|b| b.label() == label)
        .unwrap_or_else(|| panic!("{label} is not a model Endora has"));
    w.hq.set_brain(id, brain.name())
        .expect("the owner may set a member's model");
}

/// Wakes `name` to decide `title`, as the owner's answer to its question does.
#[given(expr = "{word} is woken to decide {string}")]
#[when(expr = "{word} is woken to decide {string}")]
fn woken_to_decide(w: &mut Office, name: String, title: String) {
    let id = w.member(&name);
    let task = w.a_task_nobody_has(&title);
    w.hq.wake(id, Some(task), WakeReason::ProposalDecided)
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
        w.hq.wake(id, Some(task), WakeReason::ProposalDecided)
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
        w.tick();
    }
    panic!("the pool's day would not be spent");
}

#[when("a day passes")]
fn a_day_passes(w: &mut Office) {
    w.clock.0.fetch_add(DAY_MS, Ordering::SeqCst);
    w.tick();
}

#[when(regex = r"^the owner tops up the pool by \$(\d+(?:\.\d+)?)$")]
fn tops_up_the_pool(w: &mut Office, dollars: f64) {
    w.hq.top_up_pool((dollars * DOLLAR as f64).round() as i64)
        .expect("the owner may top up the pool");
    w.tick();
}

#[then(regex = r#"^a top-up of \$(\d+(?:\.\d+)?) is refused: "(.+)"$"#)]
fn top_up_refused(w: &mut Office, dollars: f64, said: String) {
    let refused =
        w.hq.top_up_pool((dollars * DOLLAR as f64).round() as i64)
            .expect_err("the top-up is past the day's limit");
    assert!(refused.to_string().contains(&said), "{refused}");
}

/// Time goes by with the team left alone: a tick every five minutes, and any turn that started
/// is ended, as a worker that finished.
#[when(regex = r"^the team is left alone for (\d+) hours$")]
fn left_alone(w: &mut Office, hours: i64) {
    for _ in 0..hours * 12 {
        w.clock.0.fetch_add(5 * 60 * 1000, Ordering::SeqCst);
        w.tick();
        for started in w.hq.runner().started() {
            w.hq.runner().finish(started.run, true);
        }
    }
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
    w.wakes_when_waiting = w.hq.wakes().expect("the store answers").len();
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

#[then(regex = r"^(\w+)'s turn reserved nothing$")]
fn reserved_nothing(w: &mut Office, name: String) {
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
    assert_eq!((run.reserved, run.spent), (0, 0));
}

#[then(expr = "{word}'s model is {string}")]
fn model_is(w: &mut Office, name: String, model: String) {
    let id = w.member(&name);
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let on_screen = shown["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == id.0)
        .expect("on the roster");
    let brain = on_screen["brain"].as_str().expect("a brain");
    assert_eq!(
        Brain::from_name(brain).map(Brain::label),
        Some(model.as_str())
    );
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

#[then(expr = "the work on {string} is held, not stuck")]
fn held_not_stuck(w: &mut Office, title: String) {
    let task = w.task(&title);
    let found =
        w.hq.diagnosis(task)
            .expect("the store answers")
            .expect("open work");
    assert_eq!(found.stopped, Stopped::Held, "{}", found.why);
}

/// Nothing HQ does for stuck work was done for this task since its turn began to wait: no
/// recovery wake and no climb. (The ladder that asked a lead or the CEO to decide is gone.)
#[then(expr = "nobody was nudged, sent to decide or climbed to about {string}")]
fn nobody_was_nudged(w: &mut Office, title: String) {
    let task = w.task(&title);
    let woken: Vec<_> =
        w.hq.wakes()
            .expect("the store answers")
            .into_iter()
            .skip(w.wakes_when_waiting)
            .filter(|wake| wake.task == Some(task))
            .filter(|wake| {
                matches!(
                    wake.reason,
                    WakeReason::Recovery | WakeReason::AssignmentRecovery
                )
            })
            .collect();
    assert!(woken.is_empty(), "{woken:?}");
    let climbed = w
        .hq
        .store()
        .events_after(0)
        .expect("the store answers")
        .iter()
        .filter(|e| {
            matches!(e.kind.as_str(), "stuck.climbed" | "huddle.called") && e.data["task"] == task.0
        })
        .count();
    assert_eq!(climbed, 0, "the ladder was climbed for {title}");
}

#[then("the owner's desk holds no stuck work")]
fn no_stuck_work_on_the_desk(w: &mut Office) {
    let stuck: Vec<_> =
        w.hq.store()
            .desk()
            .expect("the store answers")
            .into_iter()
            .filter(|d| d.kind == "stuck")
            .collect();
    assert!(stuck.is_empty(), "{stuck:?}");
}
