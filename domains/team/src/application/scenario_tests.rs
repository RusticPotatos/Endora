//! A scenario (ADR 0135), a chess match since the other worlds were retired (2026-10-30), end
//! to end over HQ's real store and the simulated runner: started in a project that gives its
//! work the scenario tool alone, played through that tool, closed by the world and replayed
//! from its log; and the model league that seats its games.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::run_ports::RunStore;

use serde_json::json;

use super::engine::{Author, Engine, EngineError, Settings};
use super::ports::{Clock, RunContext, TeamStore};
use super::protocol::{call, tools_in};
use crate::application::receipt_ports::ReceiptStore;
use crate::domain::{MemberId, ProjectId, Refusal, Role, TaskId, TaskStatus, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 17 October 2026, at noon UTC.
const OCT_17: i64 = 1_792_238_400_000;

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(4),
        clock,
        Settings::default(),
    )
}

/// A lead, a developer on the house's model, and a Holodeck project that gives its work the
/// scenario tool alone.
fn holodeck(hq: &Hq<'_>) -> (MemberId, ProjectId) {
    let lead = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dev = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let goal = hq.set_goal("Holodeck", "", Author::Owner).unwrap();
    let project = hq.add_project(goal, "Chess", "", lead, None).unwrap();
    hq.set_project_tools(project, Some(vec!["scenario".to_owned()]))
        .unwrap();
    (dev, project)
}

fn turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(
    hq: &Hq<'_>,
    context: &RunContext,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

// G3 of the roadmap: the world changes only through its tool, in code, and a model's claim
// changes nothing. A player's turn is offered its own four and the scenario tool, nothing of
// the node's and no shell; and a member's word does not close its match.
#[test]
fn a_match_is_played_through_its_tool_alone_and_no_word_closes_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    assert_eq!(
        super::history_ports::HistoryStore::input_origin(
            hq.store(),
            crate::domain::history::InputRef::TaskBody(game)
        )
        .unwrap(),
        crate::domain::history::InputOrigin::Unknown,
        "owner-budgeted generated content is not an authenticated owner-authored input"
    );
    let [w] = <[RunContext; 1]>::try_from(turns(&hq, &[white]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&w.token).unwrap().unwrap();
    assert_eq!(run.task, Some(game));
    let mut offered: Vec<String> = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    offered.sort();
    assert_eq!(
        offered,
        ["claim", "my_turn", "post", "scenario", "set_status"]
    );
    assert!(!hq.may_use_a_shell(run.id).unwrap());
    use_tool(&hq, &w, "claim", &json!({ "task": game.0 })).unwrap();
    let claimed = use_tool(
        &hq,
        &w,
        "set_status",
        &json!({ "task": game.0, "status": "done", "reason": "I won" }),
    )
    .unwrap_err();
    assert!(claimed.contains("the world"), "{claimed}");
    assert!(!hq.store().task(game).unwrap().unwrap().status.finished());
    assert!(
        act(&hq, &w, "inspect")
            .unwrap_err()
            .contains("`action` is look, move with san")
    );
    assert_eq!(hq.scenario_of(game).unwrap(), Some(1));
    assert_eq!(hq.scenario_of(TaskId(999)).unwrap(), None);
}

// A match's work is given the scenario tool and nothing else of anyone's: a project with no
// list, or one that gives more, cannot hold one. A turn on other work is never offered it.
#[test]
fn a_scenario_starts_only_in_a_project_that_gives_it_the_tool_alone() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([dev, zed, _], project) = crew(&hq);
    for list in [
        None,
        Some(vec!["scenario".to_owned(), "homelab_status".to_owned()]),
        Some(Vec::new()),
    ] {
        hq.set_project_tools(project, list.clone()).unwrap();
        let refused = hq.start_chess(project, [dev, zed], 1).unwrap_err();
        assert!(
            matches!(&refused, EngineError::Refused(Refusal::Scenario(why)) if why.contains("scenario")),
            "{list:?}: {refused}"
        );
    }
    assert_eq!(
        hq.start_chess(ProjectId(99), [dev, zed], 1),
        Err(EngineError::Unknown("project"))
    );
    hq.set_project_tools(project, Some(vec!["scenario".to_owned()]))
        .unwrap();
    assert!(matches!(
        hq.start_chess(project, [dev, dev], 1),
        Err(EngineError::Refused(Refusal::Scenario(_)))
    ));

    let goal = hq.set_goal("Keep the house", "", Author::Owner).unwrap();
    let other = hq
        .add_task(
            Author::Owner,
            super::ports::NewTask {
                goal,
                project: None,
                parent: None,
                title: "Water the plants".to_owned(),
                description: String::new(),
                assignee: Some(dev),
                priority: crate::domain::Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap();
    let context = turn(&hq, dev);
    let (run, member) = hq.turn_of(&context.token).unwrap().unwrap();
    assert_eq!(run.task, Some(other));
    assert!(
        !tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "scenario")
    );
    assert!(
        use_tool(&hq, &context, "scenario", &json!({ "action": "look" }))
            .unwrap_err()
            .contains("no scenario")
    );
}

/// Three members on the house's models, and a Holodeck project for scenarios.
fn crew(hq: &Hq<'_>) -> ([MemberId; 3], ProjectId) {
    let (dev, project) = holodeck(hq);
    let zed = hq.hire("Zed", Role::Developer, 40, 2, 4).unwrap();
    let rae = hq.hire("Rae", Role::Developer, 40, 2, 4).unwrap();
    ([dev, zed, rae], project)
}

/// Each member's running turn, once the tick has started them.
fn turns(hq: &Hq<'_>, members: &[MemberId]) -> Vec<RunContext> {
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    let started = hq.runner().started();
    members
        .iter()
        .map(|m| {
            started
                .iter()
                .rev()
                .find(|c| c.member == *m)
                .unwrap_or_else(|| {
                    panic!(
                        "a turn of {m:?}'s: started {:?}; wakes {:?}",
                        started
                            .iter()
                            .map(|c| (c.member, c.task))
                            .collect::<Vec<_>>(),
                        hq.store()
                            .wakes()
                            .unwrap()
                            .iter()
                            .map(|w| (w.member, w.task, w.reason, w.receipt))
                            .collect::<Vec<_>>()
                    )
                })
                .clone()
        })
        .collect()
}

fn act(hq: &Hq<'_>, context: &RunContext, action: &str) -> Result<String, String> {
    use_tool(hq, context, "scenario", &json!({ "action": action }))
}

fn play(hq: &Hq<'_>, context: &RunContext, san: &str) -> Result<String, String> {
    use_tool(
        hq,
        context,
        "scenario",
        &json!({ "action": "move", "san": san }),
    )
}

// Chess as a scenario: the owner wanted to see a game move by move, which model played what,
// and the illegal moves, and none of it was kept (and no game was ever played: moves came only
// in downtime turns). A match is two members' world: a move a turn, every move tried kept, the
// side not to move told its turn is over, the other side woken by a legal move, and the rules
// close it.
#[test]
fn a_chess_match_is_played_a_move_a_turn_with_its_illegal_moves_kept() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .expect("black's part");
    assert_eq!(part.assignee, Some(black));
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&b.token).unwrap().unwrap();
    let tool = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "scenario")
        .cloned()
        .unwrap();
    assert!(tool.to_string().contains("san"), "{tool}");
    // A match's turn is offered its tool up front (ADR 0142, amended 2026-10-30).
    assert_eq!(tool["_meta"][super::protocol::UP_FRONT], true, "{tool}");

    let waiting = act(&hq, &b, "look").unwrap();
    assert!(
        waiting.contains("not your move: this turn has ended"),
        "{waiting}"
    );
    let illegal = play(&hq, &w, "e5").unwrap_err();
    assert!(
        illegal.contains("`e5` is not legal here") && !illegal.contains("e4"),
        "illegal, and nothing about which moves are: {illegal}"
    );
    assert!(!tool.to_string().contains("legal moves"), "{tool}");
    let woken_before = hq.store().wakes().unwrap().len();
    let moved = play(&hq, &w, "f3").unwrap();
    assert!(
        moved.contains("white played f3") && moved.contains("this turn has ended"),
        "{moved}"
    );
    let woken: Vec<(MemberId, Option<TaskId>)> = hq
        .store()
        .wakes()
        .unwrap()
        .into_iter()
        .skip(woken_before)
        .map(|w| (w.member, w.task))
        .collect();
    assert_eq!(woken, [(black, Some(part.id))], "the other side is woken");
    assert!(play(&hq, &w, "e4").unwrap_err().contains("black's move"));
    play(&hq, &b, "e5").unwrap();
    play(&hq, &w, "g4").unwrap();
    let mate = play(&hq, &b, "Qh4").unwrap();
    assert!(
        mate.contains("over: black_won") && mate.contains("this turn has ended"),
        "{mate}"
    );
    for task in [game, part.id] {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Done
        );
    }

    let shown = hq.scenario_shown(game).unwrap();
    assert_eq!(
        (
            &shown["kind"],
            &shown["result"],
            &shown["score"],
            &shown["moves"]
        ),
        (
            &json!("chess"),
            &json!("black_won"),
            &json!(0),
            &json!("1. f3 e5 2. g4 Qh4#")
        )
    );
    assert_eq!(shown["sides"]["white"], json!("Linus"));
    // Each move and the board after it, for the owner to step through.
    assert_eq!(shown["move_list"], json!(["f3", "e5", "g4", "Qh4#"]));
    assert_eq!(shown["positions"].as_array().unwrap().len(), 5);
    assert_eq!(shown["positions"][4], shown["rows"]);
    let by_role: Vec<(String, u64, u64)> = shown["by_role"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["role"].as_str().unwrap().to_owned(),
                r["accepted"].as_u64().unwrap(),
                r["refused"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        by_role,
        [("black".to_owned(), 2, 0), ("white".to_owned(), 2, 2)],
        "white's illegal move and its move out of turn are kept"
    );
    assert_eq!(
        hq.replay_chess(game).unwrap().move_text(),
        "1. f3 e5 2. g4 Qh4#"
    );
}

/// A match and black's part of it.
fn chess_game(hq: &Hq<'_>, project: ProjectId, players: [MemberId; 2]) -> (TaskId, TaskId) {
    let game = hq.start_chess(project, players, 1).unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .unwrap()
        .id;
    (game, part)
}

// Live, the project's lead was woken on a crew's parts by the stuck-work ladder and acted in
// their roles: a side is its task's member's, and nobody else's, whoever is woken on it.
#[test]
fn only_a_tasks_own_member_acts_in_its_role() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let ada = hq
        .store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.name == "Ada")
        .unwrap()
        .id;
    let (game, part) = chess_game(&hq, project, [white, black]);
    // The players' first turns end, so a wake on a part is not held behind its own member's.
    for c in turns(&hq, &[white, black]) {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    hq.wake(ada, Some(part), WakeReason::Mentioned).unwrap();
    let [lead] = <[RunContext; 1]>::try_from(turns(&hq, &[ada]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&lead.token).unwrap().unwrap();
    assert_eq!(run.task, Some(part));
    assert!(
        !tools_in(&hq, &run, &member)
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "scenario"),
        "not offered to her"
    );
    let refused = act(&hq, &lead, "look").unwrap_err();
    assert!(refused.contains("Zed's"), "{refused}");
    assert!(
        hq.store().scenario_actions(game).unwrap().is_empty(),
        "nothing kept as an act"
    );
}

// A match's tasks wait for their world to wake them, by design: the stuck-work guards and the
// ladder leave them alone (live, a crew's parts were blocked and the lead woken to decide).
#[test]
fn the_stuck_work_guards_leave_a_scenario_alone() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let (game, part) = chess_game(&hq, project, [white, black]);
    let contexts = turns(&hq, &[white, black]);
    for (context, task) in contexts.iter().zip([game, part]) {
        use_tool(&hq, context, "claim", &json!({ "task": task.0 })).unwrap();
    }
    for c in &contexts {
        hq.runner().finish(c.run, true);
    }
    let decided_before = hq
        .store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.reason == WakeReason::Decide)
        .count();
    for _ in 0..40 {
        clock.0.fetch_add(15 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
    }
    let statuses: Vec<TaskStatus> = [game, part]
        .iter()
        .map(|t| hq.store().task(*t).unwrap().unwrap().status)
        .collect();
    assert!(!statuses.contains(&TaskStatus::Blocked), "{statuses:?}");
    let decided = hq
        .store()
        .wakes()
        .unwrap()
        .iter()
        .filter(|w| w.reason == WakeReason::Decide)
        .count();
    assert_eq!(decided, decided_before, "no lead woken to decide it");
}

// A world that has ended with its tasks still open (as live, when its end came from a rule
// added after its last action) is closed by the tick.
#[test]
fn the_tick_closes_a_world_that_has_ended() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let (game, part) = chess_game(&hq, project, [white, black]);
    let [w] = <[RunContext; 1]>::try_from(turns(&hq, &[white]))
        .ok()
        .unwrap();
    for action in ["move f3", "move e5", "move g4", "move Qh4"] {
        hq.store()
            .add_scenario_action(&super::ports::ScenarioAction {
                id: 0,
                task: game,
                run: w.run,
                member: white,
                action: action.to_owned(),
                ok: true,
                said: String::new(),
                at: OCT_17,
            })
            .unwrap();
    }
    hq.tick().unwrap();
    for task in [game, part] {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Done,
            "a match played out is the work done"
        );
    }
    assert_eq!(hq.scenario_shown(game).unwrap()["end"], "black_won");
}

// Baseline 28 Oct: the owner paused the league, its players' wakes were held, and the idle check
// still counted the game idle, so black forfeited a game it was never let play. A paused
// world is not idle: it waits for the owner, and nobody forfeits.
#[test]
fn a_world_in_a_paused_project_is_not_idle_and_nobody_forfeits() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let [w, _b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    play(&hq, &w, "e4").unwrap();
    hq.change_project(project, "pause").unwrap();
    for _ in 0..12 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        clock.0.fetch_add(11 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
    }
    assert_ne!(
        hq.store().task(game).unwrap().unwrap().status,
        TaskStatus::Done,
        "not forfeited while paused"
    );
    assert_eq!(hq.scenario_shown(game).unwrap()["result"], json!(null));
}

// Live, black's turn on the Mac ran past its time and was stopped, the wake that would have
// brought it back had been folded into that turn, and the match waited for ever. A world whose
// player has no turn coming is woken again when it has sat idle, three times at most; then the
// world abandons it, and says so.
#[test]
fn an_idle_player_is_woken_again_three_times_and_then_the_world_abandons_it() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .unwrap();
    let [w, _b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    play(&hq, &w, "e4").unwrap();
    let woken = |hq: &Hq<'_>| {
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|x| x.member == black && x.task == Some(part.id))
            .count()
    };
    // Black's turns all end without a move; nothing else would bring it back.
    let mut counts = Vec::new();
    for _ in 0..6 {
        for started in hq.runner().started() {
            hq.runner().finish(started.run, true);
        }
        clock.0.fetch_add(11 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
        counts.push(woken(&hq));
    }
    let first = counts[0];
    assert!(
        counts.iter().max().unwrap() - first <= 3,
        "woken again three times at most: {counts:?}"
    );
    assert!(
        counts.iter().max().unwrap() > &first,
        "woken again: {counts:?}"
    );
    // A match is not dropped: the side that stopped moving forfeits, and the other wins
    // (owner, 2026-10-28: else stopping is a way out of a lost game).
    for task in [game, part.id] {
        assert_eq!(
            hq.store().task(task).unwrap().unwrap().status,
            TaskStatus::Done,
            "forfeited: {counts:?}"
        );
    }
    let shown = hq.scenario_shown(game).unwrap();
    assert_eq!(
        (&shown["result"], &shown["reason"]),
        (
            &json!("white_won"),
            &json!("black stopped moving, and forfeits")
        )
    );
    assert!(
        hq.store()
            .thread(game)
            .unwrap()
            .iter()
            .any(|p| p.text.contains("forfeits")),
        "the world says why"
    );
}

// A side may tip its king over: resign, and the other side wins.
#[test]
fn a_side_may_resign_and_the_other_wins() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    let (run, member) = hq.turn_of(&w.token).unwrap().unwrap();
    let tool = tools_in(&hq, &run, &member)
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "scenario")
        .cloned()
        .unwrap();
    assert!(
        tool["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("resign"))
    );
    play(&hq, &w, "e4").unwrap();
    let said = act(&hq, &b, "resign").unwrap();
    assert!(said.contains("over: white_won"), "{said}");
    let shown = hq.scenario_shown(game).unwrap();
    assert_eq!(
        (&shown["result"], &shown["reason"], &shown["status"]),
        (
            &json!("white_won"),
            &json!("black resigned"),
            &json!("done")
        )
    );
    // Outside the league its PGN names the members, and says who resigned.
    let pgn = hq.scenario_pgn(game).unwrap();
    for tag in [
        "[Event \"Endora Holodeck\"]",
        "[Date \"2026.10.17\"]",
        "[White \"Linus\"]",
        "[Black \"Zed\"]",
        "[Result \"1-0\"]",
        "[Termination \"normal\"]",
    ] {
        assert!(pgn.contains(tag), "{tag}: {pgn}");
    }
    assert!(pgn.ends_with("\n1. e4 {black resigned} 1-0\n"), "{pgn}");
    assert!(matches!(
        hq.scenario_pgn(TaskId(9999)),
        Err(EngineError::Unknown(_))
    ));
}

// A move resets it: a player who acts is never counted idle for its earlier silences.
#[test]
fn acting_again_resets_the_count_of_wakes() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([white, black, _], project) = crew(&hq);
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    play(&hq, &w, "e4").unwrap();
    for _ in 0..2 {
        for started in hq.runner().started() {
            if started.run != b.run {
                hq.runner().finish(started.run, true);
            }
        }
        clock.0.fetch_add(11 * 60 * 1000, Ordering::SeqCst);
        hq.tick().unwrap();
    }
    play(&hq, &b, "e5").unwrap();
    assert!(!hq.store().task(game).unwrap().unwrap().status.finished());
}

// The model league (ADR 0135, amended 2026-10-25): the owner wants the models compared at a
// game of skill, every one against every other, in a table. Each side of a league game is
// pinned to one brain whoever sits at the board; a Claude side is paid from the league's
// project, which must be capped; the next pair goes when nothing else is being played; and the
// table is rated from the results.
// Baseline 28 Oct: the owner paused the league and it went on opening games. Paused, it sets
// nobody down; resumed, it does.
#[test]
fn a_paused_league_opens_no_game() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (_, project) = crew(&hq);
    hq.set_project_cap(project, Some(5_000_000)).unwrap();
    hq.start_league(project, &["claude-haiku-5-5", "local"])
        .unwrap();
    hq.change_project(project, "pause").unwrap();
    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    assert_eq!(league["games"], json!([]), "{league}");
    hq.change_project(project, "resume").unwrap();
    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    assert!(league["games"][0]["task"].is_i64(), "{league}");
}

#[test]
fn the_model_league_pins_each_side_to_its_brain_and_rates_the_results() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (_, project) = crew(&hq);
    let brains = ["claude-haiku-5-5", "local"];
    assert!(
        hq.start_league(project, &brains).is_err(),
        "Claude plays only from a capped project"
    );
    assert!(hq.start_league(project, &["local"]).is_err(), "two brains");
    assert!(hq.start_league(project, &["local", "gpt"]).is_err());
    hq.set_project_cap(project, Some(5_000_000)).unwrap();
    hq.start_league(project, &brains).unwrap();

    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    let game = TaskId(league["games"][0]["task"].as_i64().expect("a game started"));
    assert_eq!(league["games"][0]["white"], "claude-haiku-5-5", "{league}");
    assert_eq!(league["games"][0]["black"], "local");
    let part = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(game))
        .unwrap();
    let white = hq.store().task(game).unwrap().unwrap().assignee.unwrap();
    let black = part.assignee.unwrap();
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    let ran = |c: &RunContext| hq.store().run(c.run).unwrap().unwrap();
    assert_eq!(ran(&w).brain, Some(crate::domain::Brain::Haiku));
    assert!(ran(&w).reserved > 0, "Claude's side is paid for");
    assert_eq!(ran(&b).brain, Some(crate::domain::Brain::Local));

    hq.tick().unwrap();
    assert_eq!(
        hq.snapshot().unwrap()["holodeck"]["league"]["games"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "one game at a time"
    );
    for (side, san) in [(&w, "f3"), (&b, "e5"), (&w, "g4"), (&b, "Qh4")] {
        play(&hq, side, san).unwrap();
    }
    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    assert_eq!(league["table"][0]["brain"], "local", "{league}");
    assert_eq!(league["table"][0]["rating"], 1216);
    assert_eq!(league["table"][0]["won"], 1);
    assert_eq!(league["table"][1]["rating"], 1184);
    assert_eq!(league["pairs_left"], 0);
    assert_eq!(league["games"][0]["end"], "black_won");
    // Each brain's rating after every game it finished, for the card's sparkline.
    assert_eq!(league["table"][0]["history"], json!([1200, 1216]));
    assert_eq!(league["table"][1]["history"], json!([1200, 1184]));
    // A league game's PGN is under the league, its sides the brains' labels.
    let pgn = hq.scenario_pgn(game).unwrap();
    for tag in [
        "[Event \"Endora model league\"]".to_owned(),
        "[Round \"1\"]".to_owned(),
        format!("[White \"{}\"]", crate::domain::Brain::Haiku.label()),
        format!("[Black \"{}\"]", crate::domain::Brain::Local.label()),
        "[Result \"0-1\"]".to_owned(),
    ] {
        assert!(pgn.contains(&tag), "{tag}: {pgn}");
    }
    assert!(
        pgn.ends_with("1. f3 e5 2. g4 Qh4# {checkmate} 0-1\n"),
        "{pgn}"
    );
    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    // One game a pair (the owner, 2026-10-30): the league is over, and no rematch starts.
    assert_eq!(league["games"].as_array().unwrap().len(), 1, "{league}");
}

// The relay lets a role think only with its own Claudes (ADR 0111; since one team, ADR 0142,
// Opus is any player's): a Sonnet side never seats a researcher, whose Claude is Haiku alone.
#[test]
fn a_league_side_is_played_only_by_a_member_who_may_think_with_its_brain() {
    for start in 0..8 {
        let clock = Hands(AtomicI64::new(OCT_17 + start * 7_919_000));
        let hq = engine(&clock);
        let (_, project) = holodeck(&hq);
        let researcher = hq.hire("Cy", Role::Researcher, 15, 2, 0).unwrap();
        hq.set_project_cap(project, Some(5_000_000)).unwrap();
        // Since one team (ADR 0142) Opus is no CEO's: a member may play it.
        assert!(
            hq.start_league(project, &["claude-opus-5-5", "local"])
                .is_ok()
        );
        hq.start_league(project, &["claude-sonnet-5-5", "local"])
            .unwrap();
        hq.tick().unwrap();
        let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
        let game = TaskId(league["games"][0]["task"].as_i64().expect("a game"));
        let white = hq.store().task(game).unwrap().unwrap().assignee;
        assert_ne!(white, Some(researcher), "Sonnet is never a researcher's");
    }
}

// Live, a player in the league was briefed with "what this loop did lately": its opponent's own
// words from its turns, legal moves and reasoning among them. A player in a match is told the
// game and nothing any other turn in the league said.
#[test]
fn a_player_is_never_told_what_other_turns_in_its_league_said() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([_, white, black], project) = crew(&hq);
    hq.set_model(white, "mac").unwrap();
    hq.set_model(black, "local").unwrap();
    let game = hq.start_chess(project, [white, black], 1).unwrap();
    // White's words in its own task's thread, where black's briefing used to find them.
    hq.post_as(
        white,
        game,
        "My plan: answer every c5 with Nf3 and castle short",
    )
    .unwrap();
    let [b] = <[RunContext; 1]>::try_from(turns(&hq, &[black]))
        .ok()
        .unwrap();
    assert!(!b.told.contains("My plan"), "{}", b.told);
    assert!(!b.told.contains("What this loop did lately"), "{}", b.told);
}

// The owner resets the league (2026-10-27, after the rules changed): every brain is rated
// afresh, any game being played stops, a new round starts, and the games before are kept, set
// aside, never deleted.
#[test]
fn the_owner_resets_the_league_and_it_starts_again_keeping_the_games_before() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let (_, project) = crew(&hq);
    hq.start_league(project, &["mac", "local"]).unwrap();
    hq.tick().unwrap();
    let first = TaskId(
        hq.snapshot().unwrap()["holodeck"]["league"]["games"][0]["task"]
            .as_i64()
            .unwrap(),
    );
    let white = hq.store().task(first).unwrap().unwrap().assignee.unwrap();
    let black = hq
        .store()
        .tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.parent == Some(first))
        .unwrap()
        .assignee
        .unwrap();
    let [w, b] = <[RunContext; 2]>::try_from(turns(&hq, &[white, black]))
        .ok()
        .unwrap();
    for (side, san) in [(&w, "f3"), (&b, "e5"), (&w, "g4"), (&b, "Qh4")] {
        play(&hq, side, san).unwrap();
    }
    hq.tick().unwrap();
    assert_eq!(
        hq.snapshot().unwrap()["holodeck"]["league"]["games"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "one game a pair: no rematch before a reset"
    );

    let reset = hq.reset_league().unwrap();
    assert_eq!(reset["set_aside"], 1, "{reset}");
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    assert_eq!(league["games"], json!([]), "{league}");
    assert!(
        league["table"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["rating"] == 1200 && r["won"] == 0 && r["drawn"] == 0 && r["lost"] == 0),
        "{league}"
    );
    // The finished game and its moves stay in the record.
    assert_eq!(
        hq.store().task(first).unwrap().unwrap().status,
        crate::domain::TaskStatus::Done
    );
    assert!(
        hq.store()
            .setting("league_games_before_reset")
            .unwrap()
            .unwrap()
            .contains(&first.0.to_string())
    );
    // The owner's list shows the games set aside as from before the reset, not as the new league's.
    let listed = hq.snapshot().unwrap()["holodeck"]["scenarios"].clone();
    let row = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["task"] == first.0)
        .unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(row["round"], "before the league reset", "{row}");
    // And a new round starts: once the players' turns are over, the pair sits down again.
    for c in [&w, &b] {
        hq.runner().finish(c.run, true);
    }
    hq.tick().unwrap();
    hq.tick().unwrap();
    let league = hq.snapshot().unwrap()["holodeck"]["league"].clone();
    assert_eq!(league["games"].as_array().unwrap().len(), 1, "{league}");
    assert_ne!(league["games"][0]["task"], first.0);
}

// The owner retired the Holodeck's other worlds (2026-10-30), the lighthouse and Silent Station,
// their trials and their re-runs, and kept chess. What they left in the store is kept, never
// deleted: an old world's task, its log, its parts, a queued re-run, a round's label and a
// trial are skipped wherever the Holodeck is read, and nothing errors on them; one left open
// neither holds the league back nor is touched by the tick.
#[test]
fn a_retired_worlds_rows_are_kept_and_break_nothing() {
    let clock = Hands(AtomicI64::new(OCT_17));
    let hq = engine(&clock);
    let ([linus, zed, rae], project) = crew(&hq);
    let goal = hq
        .store()
        .projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == project)
        .unwrap()
        .goal;
    let task = |title: &str, to: MemberId, parent: Option<TaskId>| {
        hq.add_task(
            Author::Owner,
            super::ports::NewTask {
                goal,
                project: Some(project),
                parent,
                title: title.to_owned(),
                description: String::new(),
                assignee: Some(to),
                priority: crate::domain::Priority::Normal,
                waits_on: Vec::new(),
            },
        )
        .unwrap()
    };
    // A lighthouse left open, and a finished mission with its parts, as the store kept them.
    let lamp = task("The dark lighthouse (seed 7)", linus, None);
    let mission = task("Silent Station (seed 3)", linus, None);
    let science = task("Silent Station: science", zed, Some(mission));
    let engineer = task("Silent Station: engineer", rae, Some(mission));
    for done in [mission, science, engineer] {
        hq.store().set_task_status(done, TaskStatus::Done).unwrap();
    }
    for (key, value) in [
        (format!("scenario:{}", lamp.0), "7".to_owned()),
        (
            format!("scenario:{}", mission.0),
            format!("silent_station_plan:3:{}:{}", science.0, engineer.0),
        ),
        (
            format!("scenario:{}", science.0),
            format!("crew:{}:science", mission.0),
        ),
        (
            format!("scenario:{}", engineer.0),
            format!("crew:{}:engineer", mission.0),
        ),
        (
            format!("scenario_round:{}", mission.0),
            "round 1".to_owned(),
        ),
        (
            "holodeck_rerun".to_owned(),
            format!("lighthouse:7:{}:0:{}", project.0, linus.0),
        ),
        ("trials".to_owned(), "[\"old\"]".to_owned()),
        (
            "trial:old".to_owned(),
            format!(
                "{{\"kind\":\"lighthouse\",\"version\":1,\"project\":{},\"seeds\":[7],\"runs\":[{{\"task\":{},\"condition\":\"a\",\"seed\":7}}]}}",
                project.0, lamp.0
            ),
        ),
    ] {
        hq.store().set_setting(&key, &value).unwrap();
    }
    for (world, action) in [
        (lamp, "inspect"),
        (mission, "plan we go"),
        (mission, "scan_station"),
    ] {
        hq.store()
            .add_scenario_action(&super::ports::ScenarioAction {
                id: 0,
                task: world,
                run: crate::domain::RunId(1),
                member: linus,
                action: action.to_owned(),
                ok: true,
                said: String::new(),
                at: OCT_17,
            })
            .unwrap();
    }

    let game = hq.start_chess(project, [zed, rae], 1).unwrap();
    let listed = hq.snapshot().unwrap()["holodeck"].clone();
    let kinds: Vec<(i64, &str)> = listed["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["task"].as_i64().unwrap(), s["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds, [(game.0, "chess")], "{listed}");
    assert!(
        listed.get("trials").is_none() && listed.get("queued").is_none(),
        "{listed}"
    );
    for old in [lamp, mission, science] {
        assert_eq!(
            hq.scenario_shown(old),
            Err(EngineError::Unknown("scenario")),
            "a retired world is no scenario now"
        );
        assert_eq!(hq.scenario_of(old).unwrap(), None);
    }

    // The tick neither closes nor restarts them, and the old log and settings stay as kept.
    hq.tick().unwrap();
    assert!(!hq.store().task(lamp).unwrap().unwrap().status.finished());
    assert_eq!(hq.store().scenario_actions(mission).unwrap().len(), 2);
    assert!(hq.store().setting("holodeck_rerun").unwrap().is_some());
    assert_eq!(
        hq.store()
            .tasks()
            .unwrap()
            .iter()
            .filter(|t| t.title.contains("lighthouse"))
            .count(),
        1,
        "no lighthouse started again"
    );

    // The league is not held back by the open lighthouse or the old queue.
    let held = hq.store().task(game).unwrap().unwrap();
    for t in hq.store().tasks().unwrap() {
        if t.id == held.id || t.parent == Some(held.id) {
            hq.store().set_task_status(t.id, TaskStatus::Done).unwrap();
        }
    }
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
    hq.tick().unwrap();
    hq.start_league(project, &["mac", "local"]).unwrap();
    hq.tick().unwrap();
    assert_eq!(
        hq.snapshot().unwrap()["holodeck"]["league"]["games"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "a league game sat down"
    );
}
