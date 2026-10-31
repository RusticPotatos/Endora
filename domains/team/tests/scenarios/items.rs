//! The steps of the one-team features (ADR 0142, amended 2026-10-30): a team of ranked models,
//! a loop with items side by side, a handover when an item changes hands, a fresh turn when one
//! runs out of room, HQ's mechanical checks at review and briefings sized to the turn; and the
//! office's chess written out in PGN and the model league (ADR 0135).

use cucumber::{given, then, when};
use endora_team::application::engine::Called;
use endora_team::application::office_ports::OfficeStore;
use endora_team::application::receipt_ports::ReceiptStore;
use endora_team::application::run_ports::RunStore;
use endora_team::application::{Author, NewTask, Submission, TeamStore};
use endora_team::domain::{Criterion, Priority, Role, RunStatus, TaskId, WakeReason};
use serde_json::json;

use super::{Office, micros};

/// What the worker posts as a turn's last word when its prompt reached its budget, after what
/// the model said on the way (ADR 0142, amended 2026-10-30: every pass fits one fresh turn).
const OUT_OF_ROOM: &str =
    "The turn's prompt reached its budget: it ends here and goes on in a fresh turn.";

impl Office {
    /// A loop item: the owner puts it on the loop's plan, for nobody yet.
    fn loop_item(&mut self, project: &str, title: &str, description: &str, criteria: &[Criterion]) {
        let goal = self.goal();
        let project = self.project(project).id;
        let task = NewTask {
            goal,
            project: Some(project),
            parent: None,
            title: title.to_owned(),
            description: description.to_owned(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        };
        // Work with criteria is handed out as the owner's console does; without, as the plan's.
        let added = if criteria.is_empty() {
            self.hq.add_task(Author::Owner, task)
        } else {
            self.hq.add_task_with(Author::Owner, task, criteria)
        };
        if self.note(added).is_none() {
            panic!("the owner puts work on a loop: {:?}", self.refused);
        }
    }

    /// Who holds `title` now, by name, as the HQ screen shows it.
    fn holder_of(&self, title: &str) -> Option<String> {
        let task = self.task(title);
        let found = self
            .hq
            .store()
            .task(task)
            .expect("the store answers")
            .expect("a task");
        let id = found.owner.or(found.assignee)?;
        self.members
            .iter()
            .find(|(_, m)| **m == id)
            .map(|(name, _)| name.clone())
    }

    /// The newest reading `name`'s running turn took, as it is cited: `r12`.
    fn newest_reading(&mut self, name: &str) -> String {
        let run = self.turn_of(name).run;
        let receipts = self
            .hq
            .store()
            .call_receipts_of(run)
            .expect("the store answers");
        let newest = receipts
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or_else(|| panic!("{name}'s turn took no reading"));
        format!("r{newest}")
    }

    /// `name` reads a container's logs in its running turn, as the node records the reading.
    fn reads_logs(&mut self, name: &str, container: &str, got: &str) {
        let context = self.turn_of(name);
        self.hq
            .record_receipt(
                &context.token,
                &Called {
                    tool: "container_logs".to_owned(),
                    args: container.to_owned(),
                    excerpt: got.to_owned(),
                    ok: true,
                    act: false,
                    confidential: false,
                },
            )
            .expect("the node's receipt");
    }

    /// `name`'s running turn ends, and HQ ticks.
    pub(super) fn ends_turn(&mut self, name: &str) {
        let context = self.turn_of(name);
        self.hq.runner().finish(context.run, true);
        self.tick();
    }

    /// The worker ends `name`'s running turn at its prompt's budget: the relay has metered
    /// `read` prompt tokens of the turn's calls, and the worker posts what the model said on the
    /// way as the turn's last word, marked as going on (only the worker sets the mark). What HQ
    /// answered is kept as the answer.
    fn runs_out_of_room(&mut self, name: &str, read: i64, said: &str) {
        let context = self.turn_of(name);
        let task = context.task.expect("a turn on an item");
        self.hq
            .note_tokens(context.run, read, 0, 0, 500)
            .expect("the relay meters the turn's calls");
        self.answer = self.use_tool(
            name,
            "post",
            &json!({ "task": task.0, "last_word": true, "continues": true,
                     "text": format!("{said}\n\n{OUT_OF_ROOM}") }),
        );
        assert!(self.answer.is_some(), "refused: {:?}", self.refused);
        self.ends_turn(name);
    }

    /// `name` asks for review of `task` in its running turn, saying `reason` and citing
    /// `evidence`.
    fn asks_for_review(&mut self, name: &str, task: TaskId, reason: &str, evidence: &str) {
        self.use_tool(
            name,
            "set_status",
            &json!({ "task": task.0, "status": "in_review", "reason": reason,
                     "evidence": [evidence] }),
        )
        .expect("the work goes to review");
    }

    /// `reviewer`'s turn on the item sends it back with `notes`, and ends.
    fn sends_back(&mut self, reviewer: &str, task: TaskId, notes: &str) {
        self.use_tool(
            reviewer,
            "review",
            &json!({ "task": task.0, "verdict": "send_back", "notes": notes }),
        )
        .expect("the reviewer sends the work back");
        self.ends_turn(reviewer);
    }
}

// --- The team and its loops -------------------------------------------------------------------

/// The owner's team since one team (ADR 0142): each member is one model, ranked by it. A title
/// is a name only; each is hired under one, as production's five were.
#[given("the team is one team of models")]
fn one_team_of_models(w: &mut Office, step: &cucumber::gherkin::Step) {
    let table = step
        .table
        .as_ref()
        .expect("the members are the step's table");
    for row in table.rows.iter().skip(1) {
        let (name, model) = (row[0].clone(), row[1].as_str());
        let role = match model {
            "opus" => Role::Ceo,
            "sonnet" | "haiku" => Role::Lead,
            _ => Role::Developer,
        };
        let id = w.hq.hire(&name, role, 10, 2, 0).expect("HQ hires");
        w.hq.set_model(id, model).expect("a model Endora has");
        w.members.insert(name, id);
    }
}

/// A loop is a project of the goal's, led by nobody in particular: the first member hired.
#[given(expr = "the loop {string} for the goal {string}")]
fn a_loop(w: &mut Office, title: String, goal: String) {
    let goal =
        w.hq.set_goal(&goal, "", Author::Owner)
            .expect("the owner sets a goal");
    w.goal = Some(goal);
    let lead =
        w.hq.store()
            .members()
            .expect("the store answers")
            .into_iter()
            .map(|m| m.id)
            .min_by_key(|id| id.0)
            .expect("a team");
    let id =
        w.hq.add_project(goal, &title, "the fork's work", lead, None)
            .expect("the owner adds the loop");
    w.projects.insert(title, id);
}

#[given(regex = r#"^the owner sets the loop "(.+)" to (\d+) items? at a time$"#)]
#[when(regex = r#"^the owner sets the loop "(.+)" to (\d+) items? at a time$"#)]
fn items_at_a_time(w: &mut Office, title: String, items: u8) {
    let project = w.project(&title).id;
    let set = w.hq.set_loop_items(project, items);
    w.note(set);
}

/// The loop's plan, a title a row; one priority, so the plan is worked in the order given.
#[given(expr = "the loop {string} has on its plan, in this order")]
fn on_its_plan(w: &mut Office, project: String, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("the plan is the step's table");
    for row in &table.rows {
        w.loop_item(&project, &row[0], "", &[]);
    }
}

#[given(regex = r#"^the loop "(.+)" has the item "(.+)", done when "(.+)"$"#)]
fn item_done_when(w: &mut Office, project: String, title: String, criterion: String) {
    w.loop_item(&project, &title, "", &[Criterion::new(&criterion, false)]);
}

#[given(regex = r#"^the loop "(.+)" has the item "(.+)": "(.+)"$"#)]
fn item_described(w: &mut Office, project: String, title: String, description: String) {
    w.loop_item(&project, &title, &description, &[]);
}

/// The owner's reply on an item wakes whoever holds it.
#[when(regex = r#"^the owner replies on "(.+)": "(.+)"$"#)]
fn owner_replies(w: &mut Office, title: String, text: String) {
    let task = w.task(&title);
    w.hq.post(Author::Owner, task, &text)
        .expect("the owner may post on any task");
}

#[given(regex = r#"^"(.+)" is (\w+)'s$"#)]
#[then(regex = r#"^"(.+)" is (\w+)'s$"#)]
fn is_held_by(w: &mut Office, title: String, name: String) {
    assert_eq!(w.holder_of(&title).as_deref(), Some(name.as_str()));
}

#[then(regex = r#"^"(.+)" is nobody's yet$"#)]
fn is_held_by_nobody(w: &mut Office, title: String) {
    assert_eq!(w.holder_of(&title), None);
}

#[then(regex = r#"^(\w+) is (not )?woken to (take|continue|review|redo) "(.+)"$"#)]
fn woken_to(w: &mut Office, name: String, not: String, why: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let reason = match why.as_str() {
        "take" => WakeReason::Assigned,
        "continue" => WakeReason::Continued,
        "review" => WakeReason::ReviewRequested,
        _ => WakeReason::SentBack,
    };
    let wakes = w.hq.wakes().expect("the store answers");
    let woken = wakes
        .iter()
        .any(|k| k.member == member && k.task == Some(task) && k.reason == reason);
    assert_eq!(
        woken,
        not.is_empty(),
        "{name} woken to {why} {title:?}: {woken}; {wakes:#?}"
    );
}

#[then(regex = r#"^(\w+) has no turn waiting on "(.+)"$"#)]
fn no_turn_waiting_on(w: &mut Office, name: String, title: String) {
    let (member, task) = (w.member(&name), w.task(&title));
    let waiting: Vec<_> =
        w.hq.store()
            .runs()
            .expect("the store answers")
            .into_iter()
            .filter(|r| r.member == member && r.task == Some(task))
            .filter(|r| matches!(r.status, RunStatus::Queued | RunStatus::RetryWaiting))
            .collect();
    assert!(waiting.is_empty(), "{name}'s turns waiting: {waiting:#?}");
}

#[then(regex = r#"^"(.+)" has (no|\d+) failures?$"#)]
fn has_failures(w: &mut Office, title: String, count: String) {
    let task = w.task(&title);
    let wanted: u32 = if count == "no" {
        0
    } else {
        count.parse().expect("a count")
    };
    assert_eq!(w.hq.loop_failures(task).expect("the store answers"), wanted);
}

// --- Work on an item ----------------------------------------------------------------------------

/// The member takes the item in its turn and reads a container's logs, as the node records it.
#[when(regex = r#"^(\w+) takes "(.+)" and reads (\w+)'s logs: "(.+)"$"#)]
fn takes_and_reads_logs(
    w: &mut Office,
    name: String,
    title: String,
    container: String,
    got: String,
) {
    let task = w.task(&title);
    w.use_tool(&name, "claim", &json!({ "task": task.0 }))
        .expect("the member takes its item");
    w.reads_logs(&name, &container, &got);
}

#[when(regex = r#"^(\w+) asks for review of "(.+)", citing (?:his|her) reading$"#)]
fn asks_for_review_citing(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let reading = w.newest_reading(&name);
    w.asks_for_review(&name, task, "Done, as the reading shows.", &reading);
    w.ends_turn(&name);
}

/// The member quotes two lines it says its newest reading holds, in backticks, as a worker does.
#[when(
    regex = r#"^(\w+) asks for review of "(.+)", quoting "(.+)" and "(.+)" from (?:his|her) reading$"#
)]
fn asks_for_review_quoting(w: &mut Office, name: String, title: String, one: String, two: String) {
    let task = w.task(&title);
    let reading = w.newest_reading(&name);
    let reason = format!("Done. {reading} shows `{one}` and `{two}`.");
    w.asks_for_review(&name, task, &reason, &reading);
    w.ends_turn(&name);
}

/// The node took the member's branch and tells HQ the files it changes, and its diff.
fn submits_changing(w: &mut Office, name: &str, branch: &str, changed: &str, diff: &str) {
    let context = w.turn_of(name);
    let changed: Vec<String> = changed.split(", ").map(str::to_owned).collect();
    let recorded = w.hq.record_branch(
        &context.token,
        &Submission {
            branch: branch.to_owned(),
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: changed.len(),
            plus: 1,
            minus: 1,
            stat: changed.iter().map(|f| format!(" {f} | 2 +-\n")).collect(),
            diff: diff.to_owned(),
            can_push: false,
            changed,
            texts: Vec::new(),
        },
    );
    w.note(recorded).expect("the node's record is taken");
}

#[when(expr = "{word} submits the branch {string}, changing {string}")]
fn submits_branch_changing(w: &mut Office, name: String, branch: String, changed: String) {
    submits_changing(w, &name, &branch, &changed, "");
}

#[when(expr = "{word} submits the branch {string}, changing {string}, with the diff")]
fn submits_branch_with_diff(
    w: &mut Office,
    name: String,
    branch: String,
    changed: String,
    step: &cucumber::gherkin::Step,
) {
    let diff = step
        .docstring
        .as_ref()
        .expect("the diff is the step's text");
    submits_changing(w, &name, &branch, &changed, diff.trim());
}

/// The reviewer one rank up sends the item back, once.
#[when(regex = r#"^(\w+) sends "(.+)" back: "(.+)"$"#)]
fn sends_back_once(w: &mut Office, reviewer: String, title: String, notes: String) {
    let task = w.task(&title);
    w.sends_back(&reviewer, task, &notes);
}

/// The item's holder asks for review and the reviewer sends it back, `times` times over: each
/// send-back is one of the item's failures, and three move it up a rank.
#[when(regex = r#"^(\w+) sends "(.+)" back (\d+) times: "(.+)"$"#)]
fn sends_back_times(w: &mut Office, reviewer: String, title: String, times: usize, notes: String) {
    let task = w.task(&title);
    for _ in 0..times {
        let worker = w.holder_of(&title).expect("the item is somebody's");
        w.asks_for_review(
            &worker,
            task,
            "Ready for a look.",
            &format!("task {}", task.0),
        );
        w.ends_turn(&worker);
        w.sends_back(&reviewer, task, &notes);
    }
}

/// A count as the owner writes it, `50,000`.
fn tokens(written: &str) -> i64 {
    written.replace(',', "").parse().expect("a count of tokens")
}

#[when(
    regex = r#"^(\w+)'s turn runs out of room after reading ([\d,]+) prompt tokens, having said "(.+)"$"#
)]
fn turn_runs_out_of_room(w: &mut Office, name: String, read: String, said: String) {
    w.runs_out_of_room(&name, tokens(&read), &said);
}

#[then(regex = r#"^the thread of "(.+)" shows Endora's handover to (\w+)$"#)]
fn thread_shows_handover(w: &mut Office, title: String, name: String) {
    let task = w.task(&title);
    let start = format!("Handover of #{} to {name} (", task.0);
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(
        thread
            .iter()
            .any(|p| p.author == "endora" && p.text.starts_with(&start)),
        "no handover to {name}: {thread:#?}"
    );
}

#[then(regex = r#"^the thread of "(.+)" says "(.+)"$"#)]
fn thread_says(w: &mut Office, title: String, text: String) {
    let task = w.task(&title);
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(
        thread.iter().any(|p| p.text.contains(&text)),
        "{text:?} is not in the thread: {thread:#?}"
    );
}

// --- Briefings -----------------------------------------------------------------------------------

/// The owner writes a section of the house page (ADR 0122).
#[given(regex = r#"^the house knows "(\w+)": "(.+)"$"#)]
fn house_knows(w: &mut Office, key: String, text: String) {
    w.hq.write_as_owner("house", &key, &key, &text)
        .expect("the owner writes the house page");
}

#[then(regex = r#"^(\w+)'s briefing does not say "(.+)"$"#)]
fn briefing_does_not_say(w: &mut Office, name: String, text: String) {
    let told = &w
        .turns
        .get(&name)
        .unwrap_or_else(|| panic!("{name}'s turn has not started"))
        .told;
    assert!(!told.contains(&text), "{name} was told:\n{told}");
}

#[then(expr = "{word}'s turn is offered none of the node's tools")]
fn offered_none_of_the_nodes(w: &mut Office, name: String) {
    let run = w.turns.get(&name).expect("the turn started").run;
    assert_eq!(
        w.hq.lab_tools_of(run).expect("the store answers"),
        Some(Vec::new())
    );
}

// --- Chess written out, and the league (ADR 0135) ----------------------------------------------

#[then("the chess game written out in PGN reads")]
fn chess_pgn_reads(w: &mut Office, step: &cucumber::gherkin::Step) {
    let wanted = step.docstring.as_ref().expect("the PGN is the step's text");
    let game =
        w.hq.store()
            .chess_games()
            .expect("the store answers")
            .pop()
            .expect("a game of chess");
    let pgn = w.hq.chess_pgn(game.id).expect("the game is written out");
    assert_eq!(pgn.trim(), wanted.trim());
}

/// A project that holds the Holodeck's scenarios: its work is given the scenario tool alone.
#[given(regex = r#"^the Holodeck project "(.+)" with a cap of \$(\d+(?:\.\d+)?) a month$"#)]
fn holodeck_project(w: &mut Office, title: String, cap: f64) {
    let goal = w.goal();
    let lead = *w.members.values().min_by_key(|id| id.0).expect("a team");
    let id =
        w.hq.add_project(goal, &title, "", lead, Some(micros(cap)))
            .expect("the owner adds the project");
    w.hq.set_project_tools(id, Some(vec!["scenario".to_owned()]))
        .expect("the owner sets its tools");
    w.projects.insert(title, id);
}

#[when(regex = r#"^the owner starts the league in "(.+)" with "(.+)" and "(.+)"$"#)]
fn starts_the_league(w: &mut Office, title: String, one: String, two: String) {
    let project = w.project(&title).id;
    let started = w.hq.start_league(project, &[&one, &two]);
    w.note(started);
}

#[then(regex = r#"^the league's table lists "(.+)" and "(.+)"$"#)]
fn league_lists(w: &mut Office, one: String, two: String) {
    let shown = w.hq.holodeck_shown().expect("the Holodeck is shown");
    let mut labels: Vec<String> = shown["league"]["table"]
        .as_array()
        .expect("a table")
        .iter()
        .filter_map(|r| r["label"].as_str().map(str::to_owned))
        .collect();
    labels.sort();
    let mut wanted = vec![one, two];
    wanted.sort();
    assert_eq!(labels, wanted);
}

/// Turns that show nothing, ten minutes apart, each asked for by the owner's word: three of
/// them are one of the item's failures, and three failures move it up a rank (ADR 0142).
#[when(regex = r#"^(\w+) takes (\d+) more turns on "(.+)" that show nothing$"#)]
fn turns_that_show_nothing(w: &mut Office, name: String, turns: usize, title: String) {
    let task = w.task(&title);
    for _ in 0..turns {
        w.clock
            .0
            .fetch_add(10 * 60_000, std::sync::atomic::Ordering::SeqCst);
        w.hq.post(Author::Owner, task, "Any news?")
            .expect("the owner may post on any task");
        w.use_tool(
            &name,
            "post",
            &json!({ "task": task.0, "text": "Looking." }),
        )
        .expect("a member may post on its item");
        w.ends_turn(&name);
    }
}
