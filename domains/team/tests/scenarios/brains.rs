//! The steps of `features/the-two-brains.feature` (ADR 0131): a member works with the Mac or the
//! house's model and judges with a Claude, and HQ decides from why a turn was asked for which of
//! them it runs on.

use std::sync::atomic::Ordering;

use endora_team::application::run_ports::RunStore;

use cucumber::{given, then, when};
use endora_team::application::{Author, Engine, NewTask, TeamStore};
use endora_team::domain::guards::Stopped;
use endora_team::domain::money::{DAY_MS, DOLLAR, LEAST_A_TURN};
use endora_team::domain::{Brain, Priority, Receipt, Refused, Role, RunStatus, TaskId, WakeReason};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};
use serde_json::json;

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

/// A member hired on the house's model, as owners hired before members had two brains.
#[given(expr = "{word} is hired as a {word} on the house's model")]
fn hired_on_the_house_model(w: &mut Office, name: String, role: String) {
    let role = if role == "tester" {
        Role::Reviewer
    } else {
        Role::from_name(&role).unwrap_or_else(|| panic!("the team has no role called {role}"))
    };
    let hired = w.hq.hire_by_owner(&name, role, "local", None);
    if let Some(id) = w.note(hired) {
        w.members.insert(name, id);
    }
}

#[given(expr = "{word} judges with {string}")]
#[when(expr = "the owner gives {word} {string} to judge with")]
fn judges_with(w: &mut Office, name: String, label: String) {
    let id = w.member(&name);
    let role = w.the_member(&name).role;
    let brain = Brain::judgment_choices(role)
        .into_iter()
        .find(|b| b.label() == label)
        .unwrap_or_else(|| panic!("{label} is not a Claude a {role:?} may judge with"));
    w.hq.set_judgment_brain(id, brain.name())
        .expect("the owner may choose a judgment brain");
}

#[given(expr = "{word} works with the Mac")]
fn works_with_the_mac(w: &mut Office, name: String) {
    let id = w.member(&name);
    w.hq.set_work_brain(id, "mac")
        .expect("the Mac may be worked with");
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

/// `name` is asked to review `title`, as the engine asks a tester when work is handed in.
#[when(expr = "{word} is asked to review {string}")]
fn asked_to_review(w: &mut Office, name: String, title: String) {
    let id = w.member(&name);
    let task = w.a_task_nobody_has(&title);
    w.hq.wake(id, Some(task), WakeReason::ReviewRequested)
        .expect("a wake has a receipt");
}

/// `name`'s work on `title` is sent back: mending it is code work, whoever does it.
#[when(expr = "{word} is sent back {string}")]
fn is_sent_back(w: &mut Office, name: String, title: String) {
    let id = w.member(&name);
    let task = w.task(&title);
    w.hq.wake(id, Some(task), WakeReason::SentBack)
        .expect("a wake has a receipt");
}

/// The alarm goes off for `name`, about a task of its own, as Endora raises it into a routine.
#[when(expr = "an alarm {string} goes off for {word}")]
fn an_alarm_goes_off(w: &mut Office, title: String, name: String) {
    let id = w.member(&name);
    let task = w.a_task_nobody_has(&title);
    w.hq.wake(id, Some(task), WakeReason::Alarm)
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

/// `name` takes the task it was handed, as its turn would with the claim tool.
#[when(expr = "{word} takes {string}")]
fn takes(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its own task");
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

#[then(expr = "{word} works with {string} and judges with {string}")]
fn works_and_judges_with(w: &mut Office, name: String, works: String, judges: String) {
    let id = w.member(&name);
    let shown = w.hq.snapshot().expect("the snapshot is built");
    let on_screen = shown["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == id.0)
        .expect("on the roster");
    assert_eq!(on_screen["work_brain_label"], works.as_str());
    assert_eq!(on_screen["judgment_brain_label"], judges.as_str());
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

#[then(expr = "the owner's desk has a notice saying {string}")]
fn the_desk_has_a_notice(w: &mut Office, part: String) {
    let said: Vec<String> =
        w.hq.store()
            .desk()
            .expect("the store answers")
            .into_iter()
            .filter(|d| d.kind == "notice" && d.status == "waiting")
            .map(|d| d.text)
            .collect();
    assert!(
        said.iter().any(|text| text.contains(&part)),
        "no notice says {part:?}: {said:?}"
    );
}

#[then(regex = r"^the owner's desk has (\d+) notices? saying (.+)$")]
fn the_desk_has_n_notices(w: &mut Office, count: usize, part: String) {
    let part = part.trim_matches('"').to_owned();
    let said =
        w.hq.store()
            .desk()
            .expect("the store answers")
            .into_iter()
            .filter(|d| d.kind == "notice" && d.text.contains(&part))
            .count();
    assert_eq!(said, count, "notices saying {part:?}");
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
/// recovery wake, no look from its lead, no decision asked of a lead or the CEO, no huddle, no
/// finding. (A decision asked of the member who holds it is the test's own, not the ladder's.)
#[then(expr = "nobody was nudged, sent to decide or climbed to about {string}")]
fn nobody_was_nudged(w: &mut Office, title: String) {
    let task = w.task(&title);
    let members = w.hq.store().members().expect("the store answers");
    let woken: Vec<_> = w
        .hq
        .wakes()
        .expect("the store answers")
        .into_iter()
        .skip(w.wakes_when_waiting)
        .filter(|wake| wake.task == Some(task))
        .filter(|wake| {
            let role = members.iter().find(|m| m.id == wake.member).map(|m| m.role);
            matches!(
                wake.reason,
                WakeReason::Recovery | WakeReason::AssignmentRecovery
            ) || (wake.reason == WakeReason::Decide && matches!(role, Some(Role::Lead | Role::Ceo)))
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
