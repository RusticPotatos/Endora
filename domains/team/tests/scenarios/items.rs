//! The steps of the one-team features (ADR 0142, amended 2026-10-30): a team of ranked models,
//! a loop with items side by side, a handover when an item changes hands, a fresh turn when one
//! runs out of room, HQ's mechanical checks at review and briefings sized to the turn; and the
//! office's chess written out in PGN and the model league (ADR 0135).

use cucumber::{given, then, when};
use endora_team::application::office_ports::OfficeStore;
use endora_team::application::receipt_ports::ReceiptStore;
use endora_team::application::run_ports::RunStore;
use endora_team::application::wing_ports::WingStore;
use endora_team::application::{Author, Submission, TeamStore};
use endora_team::domain::{Criterion, RunStatus, TaskId, WakeReason};
use serde_json::json;

use super::{Office, micros};

/// What the worker posts as a turn's last word when its prompt reached its budget, after what
/// the model said on the way (ADR 0142, amended 2026-10-30: every pass fits one fresh turn).
const OUT_OF_ROOM: &str =
    "The turn's prompt reached its budget: it ends here and goes on in a fresh turn.";

impl Office {
    /// The newest reading `name`'s running turn took, as it is cited: `r12`.
    pub(super) fn newest_reading(&mut self, name: &str) -> String {
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
        w.hires_model(&row[0], &row[1]);
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
    w.makes_loop(&title, lead);
}

/// A loop for `goal` led by `lead`: whoever leads a loop under "Improve the lab" is of the
/// engineering wing (ADR 0115).
#[given(expr = "the loop {string} for the goal {string}, led by {word}")]
fn a_loop_led_by(w: &mut Office, title: String, goal: String, lead: String) {
    let goal =
        w.hq.set_goal(&goal, "", Author::Owner)
            .expect("the owner sets a goal");
    w.goal = Some(goal);
    let lead = w.member(&lead);
    w.makes_loop(&title, lead);
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
    let wanted: u32 = if count == "no" {
        0
    } else {
        count.parse().expect("a count")
    };
    assert_eq!(w.failures(&title), wanted);
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
    w.reads(&name, "container_logs", &container, &got);
}

#[when(regex = r#"^(\w+) asks for review of "(.+)", citing (?:his|her) reading$"#)]
fn asks_for_review_citing(w: &mut Office, name: String, title: String) {
    let task = w.task(&title);
    let reading = w.newest_reading(&name);
    w.asks_for_review(
        &name,
        task,
        "Done, as the reading shows.",
        &json!([reading]),
    );
}

/// The member quotes two lines it says its newest reading holds, in backticks, as a worker does.
#[when(
    regex = r#"^(\w+) asks for review of "(.+)", quoting "(.+)" and "(.+)" from (?:his|her) reading$"#
)]
fn asks_for_review_quoting(w: &mut Office, name: String, title: String, one: String, two: String) {
    let task = w.task(&title);
    let reading = w.newest_reading(&name);
    let reason = format!("Done. {reading} shows `{one}` and `{two}`.");
    w.asks_for_review(&name, task, &reason, &json!([reading]));
}

/// The node took the member's branch and tells HQ the files it changes, and its diff.
fn submits_changing(w: &mut Office, name: &str, branch: &str, changed: &str, diff: &str) {
    let context = w.turn_of(name);
    let changed: Vec<String> = changed.split(", ").map(str::to_owned).collect();
    // Each submission is a new commit, as a fix after a failed check is: its own head, the
    // same first twelve characters. One revision submitted again records nothing new.
    let made = w.hq.store().branches().expect("the store answers").len();
    let recorded = w.hq.record_branch(
        &context.token,
        &Submission {
            branch: branch.to_owned(),
            head: format!("a1b2c3d4e5f6{:012x}", made + 1),
            base: "0123456789abcdef01234567".to_owned(),
            files: changed.len(),
            plus: 1,
            minus: 1,
            stat: changed.iter().map(|f| format!(" {f} | 2 +-\n")).collect(),
            diff: diff.to_owned(),
            can_push: false,
            changed,
            texts: Vec::new(),
            workflows: None,
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
    w.reviews(&reviewer, task, false, &notes);
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
            &json!([format!("task {}", task.0)]),
        );
        w.reviews(&reviewer, task, false, &notes);
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
    w.runs_out_of_room(&name, tokens(&read), &format!("{said}\n\n{OUT_OF_ROOM}"));
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

#[then(regex = r#"^the thread of "(.+)" does not say "(.+)"$"#)]
fn thread_does_not_say(w: &mut Office, title: String, text: String) {
    let task = w.task(&title);
    let thread = w.hq.store().thread(task).expect("the store answers");
    assert!(
        !thread.iter().any(|p| p.text.contains(&text)),
        "{text:?} is in the thread: {thread:#?}"
    );
}

/// How HQ kept a task's closing (ADR 0124): its reason's name, and the reason as the owner reads
/// it.
#[then(regex = r#"^"(.+)" closed as "(\w+)": "(.+)"$"#)]
fn closed_as(w: &mut Office, title: String, why: String, said: String) {
    use endora_team::application::guard_ports::GuardStore;
    let task = w.task(&title);
    let closed =
        w.hq.store()
            .closure(task)
            .expect("the store answers")
            .expect("it closed with a reason");
    assert_eq!(closed.closure.name(), why, "{closed:?}");
    assert_eq!(closed.closure.plainly(), said, "{closed:?}");
}

/// The feed's word that the task is done, and whether it says nothing checked it.
#[then(regex = r#"^the feed says "(.+)" is done, (verified|not verified)$"#)]
fn feed_says_done(w: &mut Office, title: String, how: String) {
    let task = w.task(&title);
    let done =
        w.hq.events_after(0)
            .expect("the feed answers")
            .into_iter()
            .rfind(|e| {
                e.kind == "task.changed" && e.data["task"] == task.0 && e.data["status"] == "done"
            })
            .expect("the feed says it is done");
    assert_eq!(
        done.data["unverified"] == true,
        how == "not verified",
        "{:?}",
        done.data
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

/// White, in the league's game in play, resigns in its turn; then every turn under way ends,
/// so the players are free for the next game.
#[when("white resigns the league's game in play")]
fn white_resigns(w: &mut Office) {
    w.tick();
    let league = w.hq.holodeck_shown().expect("the Holodeck is shown")["league"].clone();
    let game = league["games"]
        .as_array()
        .expect("the league's games")
        .iter()
        .find(|g| g["live"] == true)
        .and_then(|g| g["task"].as_i64())
        .unwrap_or_else(|| panic!("no league game is in play: {league}"));
    let white =
        w.hq.store()
            .task(TaskId(game))
            .expect("the store answers")
            .and_then(|t| t.assignee)
            .expect("white is seated");
    let name = w
        .members
        .iter()
        .find(|(_, id)| **id == white)
        .map(|(n, _)| n.clone())
        .expect("white is on the team");
    let said = w.use_tool(&name, "scenario", &json!({ "action": "resign" }));
    assert!(said.is_some(), "{name} could not resign: {:?}", w.refused);
    for run in w.hq.store().live_runs().expect("the store answers") {
        w.hq.runner().finish(run.id, true);
    }
    w.tick();
}

#[when("the owner resets the league")]
fn resets_the_league(w: &mut Office) {
    let reset = w.hq.reset_league();
    w.note(reset);
}

#[then("the league's finished games read, newest first")]
fn finished_games_read(w: &mut Office, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("the games are the step's table");
    let page = w.hq.league_games(1, 10).expect("the games are listed");
    let read: Vec<Vec<String>> = page["games"]
        .as_array()
        .expect("a page of games")
        .iter()
        .map(|g| {
            [
                "round",
                "white_label",
                "black_label",
                "result",
                "half_moves",
            ]
            .iter()
            .map(|k| match &g[*k] {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect()
        })
        .collect();
    assert_eq!(read, table.rows[1..], "{page}");
}

#[then(regex = r#"^the league's round is (\d+), since a reset on "(.+)"$"#)]
fn the_round(w: &mut Office, number: u64, day: String) {
    let league = w.hq.holodeck_shown().expect("the Holodeck is shown")["league"].clone();
    assert_eq!(
        league["round"],
        json!({ "number": number, "since": day }),
        "{league}"
    );
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
