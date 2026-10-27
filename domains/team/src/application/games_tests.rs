//! Games in the office (ADR 0121): ping-pong and pool that the engine plays in code between
//! free members, which costs nothing and never asks a model, and chess, played by the rules in
//! downtime turns.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::application::office_ports::OfficeStore;

use crate::application::run_ports::RunStore;

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, RunContext, TeamStore};
use super::protocol::{call, tools};
use crate::application::event_ports::EventStore;
use crate::domain::chess::{self, Status};
use crate::domain::{MemberId, Role, RunStatus, WakeReason};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

/// 16 Oct 2026, 16:00 UTC.
const START: i64 = 1_792_166_400_000;
const MINUTE: i64 = 60 * 1000;
const HOUR: i64 = 60 * MINUTE;

fn engine(clock: &Hands, downtime: bool, games: bool) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            downtime,
            games,
            ..Settings::default()
        },
    )
}

struct Team {
    ceo: MemberId,
    ada: MemberId,
    bo: MemberId,
    cy: MemberId,
    dee: MemberId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let bo = hq.hire("Bo", Role::Developer, 40, 2, 4).unwrap();
    let cy = hq.hire("Cy", Role::Researcher, 20, 1, 1).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 1, 2).unwrap();
    Team {
        ceo,
        ada,
        bo,
        cy,
        dee,
    }
}

fn run_of(hq: &Hq<'_>, context: &RunContext) -> crate::application::ports::Run {
    hq.store().run(context.run).unwrap().unwrap()
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, who) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &who, tool, args)
}

fn chess_tool(hq: &Hq<'_>, context: &RunContext, args: &Value) -> Result<String, String> {
    use_tool(hq, context, "chess", args)
}

/// A turn of `member`'s about no task, started by a mention, ready for tool calls.
fn mention_turn(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.wake(member, None, WakeReason::Mentioned).unwrap();
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn")
}

fn end_all(hq: &Hq<'_>) {
    for context in hq.runner().started() {
        hq.runner().finish(context.run, true);
    }
    hq.tick().unwrap();
}

fn events(hq: &Hq<'_>, kind: &str) -> Vec<Value> {
    hq.store()
        .events_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.data)
        .collect()
}

/// Every downtime turn started so far.
fn downtime_turns(hq: &Hq<'_>) -> Vec<RunContext> {
    let wakes = hq.store().wakes().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .filter(|c| {
            let run = run_of(hq, c);
            wakes
                .iter()
                .any(|w| w.id == run.wake && w.reason == WakeReason::Downtime)
        })
        .collect()
}

/// Lets time pass until `member` has a downtime turn running, and returns it.
fn downtime_for(hq: &Hq<'_>, clock: &Hands, member: MemberId) -> RunContext {
    for _ in 0..40 {
        let running = |hq: &Hq<'_>| {
            downtime_turns(hq)
                .into_iter()
                .rfind(|c| c.member == member && run_of(hq, c).status == RunStatus::Running)
        };
        if let Some(found) = running(hq) {
            return found;
        }
        end_all(hq);
        if let Some(found) = running(hq) {
            return found;
        }
        clock.0.fetch_add(13 * HOUR, Ordering::SeqCst);
        hq.tick().unwrap();
    }
    panic!("no downtime turn for member {}", member.0);
}

/// Only these two are at work: the rest are paused.
fn only(hq: &Hq<'_>, t: &Team, keep: [MemberId; 2]) {
    for member in [t.ada, t.bo, t.cy, t.dee] {
        if !keep.contains(&member) {
            hq.pause(member, true).unwrap();
        }
    }
}

// ---- ping-pong and pool -------------------------------------------------------------------

#[test]
fn the_snapshot_carries_chess_and_no_tracked_ping_pong_or_pool() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, true);
    let t = team(&hq);
    hq.tick().unwrap();
    let chess_id = hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let shown = hq.snapshot().unwrap()["games"].clone();
    assert!(shown.get("ping_pong").is_none() && shown.get("pool").is_none());
    // The game HQ set free members, and this one.
    let all_live = shown["chess"]["games"].as_array().unwrap();
    assert_eq!(all_live.len(), 2);
    let live: Vec<&Value> = all_live.iter().filter(|g| g["id"] == chess_id).collect();
    assert_eq!(live[0]["id"], chess_id);
    assert_eq!(live[0]["white"], "Ada");
    assert_eq!(live[0]["black"], "Bo");
    assert_eq!(live[0]["status"], "in_progress");
    assert_eq!(live[0]["to_move"], "Ada");
    assert_eq!(live[0]["rows"].as_array().unwrap().len(), 8);
    assert!(shown["chess"]["leaderboard"].as_array().unwrap().is_empty());
}

// ---- chess --------------------------------------------------------------------------------

#[test]
fn every_role_is_offered_the_chess_tool() {
    for role in [
        Role::Lead,
        Role::Developer,
        Role::Researcher,
        Role::Reviewer,
    ] {
        let all = tools();
        let tool = all
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "chess")
            .unwrap_or_else(|| panic!("{role:?} lacks the chess tool"));
        let actions = tool["inputSchema"]["properties"]["action"]["enum"].clone();
        assert_eq!(actions, json!(["board", "move", "challenge"]));
    }
}

#[test]
fn a_member_challenges_another_in_a_downtime_turn_and_the_game_begins() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    hq.tick().unwrap();
    let ada = downtime_for(&hq, &clock, t.ada);
    let said = chess_tool(&hq, &ada, &json!({ "action": "challenge", "member": "bo" })).unwrap();
    assert!(said.contains("Bo") && said.contains("white"), "{said}");
    let all = hq.store().chess_games().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].white, all[0].black), (t.ada, t.bo));
    assert!(all[0].live() && all[0].moves.is_empty());
    let started = events(&hq, "chess.started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0]["text"], "Ada challenges Bo to chess");
    // The challenger is told the board, and may move at once.
    let board = chess_tool(&hq, &ada, &json!({ "action": "board" })).unwrap();
    assert!(board.contains("Your move"), "{board}");
}

#[test]
fn one_game_each_and_only_a_member_at_hand_can_be_challenged() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    hq.pause(t.cy, true).unwrap();
    hq.store().furlough_member(t.dee, START).unwrap();
    let ada = downtime_for(&hq, &clock, t.ada);
    let challenge =
        |who: &str| chess_tool(&hq, &ada, &json!({ "action": "challenge", "member": who }));
    assert!(challenge("Ada").unwrap_err().contains("yourself"));
    assert!(challenge("Nobody").unwrap_err().contains("nobody"));
    assert!(
        challenge("Mara").unwrap_err().contains("not at hand"),
        "the CEO"
    );
    assert!(
        challenge("Cy").unwrap_err().contains("not at hand"),
        "paused"
    );
    assert!(
        challenge("Dee").unwrap_err().contains("not at hand"),
        "furloughed"
    );
    challenge("Bo").unwrap();
    // Ada has a game now; nobody may start a second with her, and she may not start another.
    let again = challenge("Bo").unwrap_err();
    assert!(again.contains("in a game of chess already"), "{again}");
    assert_eq!(hq.store().chess_games().unwrap().len(), 1);
    hq.pause(t.cy, false).unwrap();
    let busy = challenge("Cy").unwrap_err();
    assert!(busy.contains("already"), "{busy}");
    let _ = t.ceo;
}

#[test]
fn the_board_shows_the_position_whose_turn_it_is_and_the_last_moves_but_never_the_legal_moves() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    only(&hq, &t, [t.ada, t.bo]);
    let id = hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let mut game = hq.store().chess_games().unwrap().remove(0);
    for (i, m) in ["e4", "e5", "Nf3"].iter().enumerate() {
        let mover = if i % 2 == 0 { t.ada } else { t.bo };
        game.play(mover, m, START + 1).unwrap();
    }
    hq.store().save_chess_game(&game).unwrap();
    assert_eq!(id, game.id);
    let bo = downtime_for(&hq, &clock, t.bo);
    let board = chess_tool(&hq, &bo, &json!({ "action": "board" })).unwrap();
    for said in [
        "Ada (white) against Bo (black)",
        "Your move",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2",
        "8 r n b q k b n r 8",
        "1. e4 e5 2. Nf3",
    ] {
        assert!(board.contains(said), "{said}\n---\n{board}");
    }
    assert!(!board.contains("egal"), "{board}");
    // The opponent sees it is not their move, and is not shown legal moves to play.
    let ada = downtime_for(&hq, &clock, t.ada);
    let theirs = chess_tool(&hq, &ada, &json!({ "action": "board" })).unwrap();
    assert!(theirs.contains("Waiting for Bo"), "{theirs}");
    assert!(!theirs.contains("Your move"), "{theirs}");
}

#[test]
fn a_move_is_played_only_on_your_turn_and_only_if_legal() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    only(&hq, &t, [t.ada, t.bo]);
    hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let bo = downtime_for(&hq, &clock, t.bo);
    // It is Ada's move, not Bo's.
    let refused = chess_tool(&hq, &bo, &json!({ "action": "move", "san": "e5" })).unwrap_err();
    assert!(refused.contains("not your move"), "{refused}");
    hq.runner().finish(bo.run, true);
    let ada = downtime_for(&hq, &clock, t.ada);
    // Not legal: refused, saying only that.
    let illegal = chess_tool(&hq, &ada, &json!({ "action": "move", "san": "e5" })).unwrap_err();
    assert!(
        illegal.contains("`e5` is not legal here") && !illegal.contains("e4"),
        "{illegal}"
    );
    let nonsense =
        chess_tool(&hq, &ada, &json!({ "action": "move", "san": "castle" })).unwrap_err();
    assert!(nonsense.contains("not a move"), "{nonsense}");
    assert!(hq.store().chess_games().unwrap()[0].moves.is_empty());
    // A legal one is played, said in the feed, and then it is Bo's.
    let played = chess_tool(&hq, &ada, &json!({ "action": "move", "san": "e4" })).unwrap();
    assert!(
        played.contains("e4") && played.contains("Waiting for Bo"),
        "{played}"
    );
    assert_eq!(hq.store().chess_games().unwrap()[0].moves, ["e4"]);
    let moved = events(&hq, "chess.moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(
        moved[0]["text"],
        "Ada played e4 against Bo at chess (move 1)"
    );
    // Once a turn: a second move by Ada is not her turn.
    let again = chess_tool(&hq, &ada, &json!({ "action": "move", "san": "d4" })).unwrap_err();
    assert!(again.contains("not your move"), "{again}");
    // And Bo, in a later downtime turn, answers.
    hq.runner().finish(ada.run, true);
    let bo = downtime_for(&hq, &clock, t.bo);
    chess_tool(&hq, &bo, &json!({ "action": "move", "san": "e5" })).unwrap();
    assert_eq!(hq.store().chess_games().unwrap()[0].moves, ["e4", "e5"]);
}

#[test]
fn chess_is_played_in_any_turn_on_the_houses_brains_and_never_on_claude() {
    // Chess was played only in downtime, which a member holding any open task never has: in
    // four days, nine downtime turns, and no game. Now any turn on the house's own brains (its
    // model or the Mac) may look, challenge and move; a turn on Claude, which costs money, may
    // only look.
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, false);
    let t = team(&hq);
    let ordinary = mention_turn(&hq, t.ada);
    assert!(
        !run_of(&hq, &ordinary)
            .brain
            .is_some_and(crate::domain::Brain::cloud),
        "a work turn is on the house's brains"
    );
    let none = chess_tool(&hq, &ordinary, &json!({ "action": "board" })).unwrap_err();
    assert!(none.contains("no game of chess"), "{none}");
    chess_tool(
        &hq,
        &ordinary,
        &json!({ "action": "challenge", "member": "Bo" }),
    )
    .unwrap();
    let said = chess_tool(&hq, &ordinary, &json!({ "action": "move", "san": "e4" })).unwrap();
    assert!(said.contains("You played e4"), "{said}");
    assert_eq!(hq.store().chess_games().unwrap()[0].moves, ["e4"]);

    // A turn on Claude may look, not play.
    let cy = mention_turn(&hq, t.cy);
    let mut on_claude = run_of(&hq, &cy);
    on_claude.brain = Some(crate::domain::Brain::Haiku);
    hq.store().save_run(&on_claude).unwrap();
    let refused =
        chess_tool(&hq, &cy, &json!({ "action": "challenge", "member": "Dee" })).unwrap_err();
    assert!(refused.contains("never in one on Claude"), "{refused}");
    assert_eq!(hq.store().chess_games().unwrap().len(), 1);
}

#[test]
fn a_game_is_picked_up_over_time_by_the_member_whose_move_it_is() {
    // The member to move gets a short turn on the house's model when nothing else is coming for
    // it, open tasks or not, and at most every half hour: games go on over days.
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, true);
    let t = team(&hq);
    only(&hq, &t, [t.ada, t.bo]);
    hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let mut game = hq.store().chess_games().unwrap().remove(0);
    game.play(t.ada, "e4", START + 1).unwrap();
    hq.store().save_chess_game(&game).unwrap();
    let goal = hq.set_goal("Keep the house", "", Author::Owner).unwrap();
    hq.add_task(
        Author::Owner,
        crate::application::ports::NewTask {
            goal,
            project: None,
            parent: None,
            title: "An old task Bo holds".to_owned(),
            description: String::new(),
            assignee: Some(t.bo),
            priority: crate::domain::Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    end_all(&hq);
    let chess_wakes = |hq: &Hq<'_>, who: MemberId| {
        hq.store()
            .wakes()
            .unwrap()
            .iter()
            .filter(|w| w.member == who && w.reason == WakeReason::Downtime)
            .count()
    };
    let before = chess_wakes(&hq, t.bo);
    clock.0.fetch_add(31 * MINUTE, Ordering::SeqCst);
    end_all(&hq);
    assert_eq!(
        chess_wakes(&hq, t.bo),
        before + 1,
        "Bo's move: a turn for it"
    );
    assert_eq!(chess_wakes(&hq, t.ada), 0, "not Ada's move");
    clock.0.fetch_add(10 * MINUTE, Ordering::SeqCst);
    end_all(&hq);
    assert_eq!(
        chess_wakes(&hq, t.bo),
        before + 1,
        "not again within the half hour"
    );
    clock.0.fetch_add(25 * MINUTE, Ordering::SeqCst);
    end_all(&hq);
    assert_eq!(
        chess_wakes(&hq, t.bo),
        before + 2,
        "still his move: picked up again"
    );
}

#[test]
fn checkmate_ends_the_game_says_so_and_goes_on_the_leaderboard() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    only(&hq, &t, [t.ada, t.bo]);
    hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let mut game = hq.store().chess_games().unwrap().remove(0);
    for (i, m) in ["e4", "e5", "Bc4", "Nc6", "Qh5", "Nf6"].iter().enumerate() {
        let mover = if i % 2 == 0 { t.ada } else { t.bo };
        game.play(mover, m, START + 1).unwrap();
    }
    hq.store().save_chess_game(&game).unwrap();
    let ada = downtime_for(&hq, &clock, t.ada);
    let said = chess_tool(&hq, &ada, &json!({ "action": "move", "san": "Qxf7" })).unwrap();
    assert!(
        said.contains("Qxf7#") && said.contains("checkmate"),
        "{said}"
    );
    let kept = &hq.store().chess_games().unwrap()[0];
    assert_eq!(kept.status, Status::WhiteWon);
    assert_eq!(kept.reason, "checkmate");
    let ended = events(&hq, "chess.ended");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0]["text"], "Ada beat Bo at chess by checkmate");
    // Over: no more moves, and both may start a new game.
    let after = chess_tool(&hq, &ada, &json!({ "action": "move", "san": "a3" })).unwrap_err();
    assert!(
        after.contains("no game of chess") || after.contains("over"),
        "{after}"
    );
    chess_tool(&hq, &ada, &json!({ "action": "challenge", "member": "Bo" })).unwrap();
    assert_eq!(hq.store().chess_games().unwrap().len(), 2);
    let board = hq.snapshot().unwrap()["games"]["chess"]["leaderboard"].clone();
    assert_eq!(board[0]["name"], "Ada");
    assert_eq!(
        (board[0]["wins"].clone(), board[0]["losses"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(board[1]["name"], "Bo");
    assert_eq!(board[1]["losses"], 1);
}

#[test]
fn a_game_nobody_moves_in_for_a_week_is_adjourned_as_a_draw() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, false);
    let t = team(&hq);
    hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    clock
        .0
        .store(START + chess::ADJOURN_AFTER_MS - 1, Ordering::SeqCst);
    hq.tick().unwrap();
    assert!(hq.store().chess_games().unwrap()[0].live());
    clock
        .0
        .store(START + chess::ADJOURN_AFTER_MS, Ordering::SeqCst);
    hq.tick().unwrap();
    let kept = &hq.store().chess_games().unwrap()[0];
    assert_eq!(kept.status, Status::Draw);
    assert!(kept.reason.contains("adjourned"));
    let ended = events(&hq, "chess.ended");
    assert_eq!(ended.len(), 1);
    assert!(
        ended[0]["text"].as_str().unwrap().contains("drew at chess"),
        "{ended:?}"
    );
    hq.tick().unwrap();
    assert_eq!(events(&hq, "chess.ended").len(), 1, "once");
}

#[test]
fn a_downtime_briefing_says_whose_move_it_is_in_chess() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    only(&hq, &t, [t.ada, t.bo]);
    hq.store().add_chess_game(t.ada, t.bo, START).unwrap();
    let ada = downtime_for(&hq, &clock, t.ada);
    assert!(
        ada.told.contains("Your move against Bo in chess"),
        "{}",
        ada.told
    );
    assert!(ada.told.contains("chess"), "{}", ada.told);
    // Live, the house's model offered the office as well left it alone, turn after turn, and
    // never moved: a turn whose move is due is about the move alone (2026-10-25).
    assert!(
        !ada.told.contains("Leaving the office as it is") && !ada.told.contains("You may place"),
        "{}",
        ada.told
    );
    assert!(
        ada.told.contains("This turn is for your move"),
        "{}",
        ada.told
    );
    hq.runner().finish(ada.run, true);
    let bo = downtime_for(&hq, &clock, t.bo);
    assert!(bo.told.contains("waiting for Ada"), "{}", bo.told);
    assert!(!bo.told.contains("Your move against"), "{}", bo.told);
    assert!(bo.told.len() < 4_000, "keep it short: {}", bo.told.len());
}

#[test]
fn a_member_without_a_game_is_told_it_may_challenge_someone() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, true, false);
    let t = team(&hq);
    let ada = downtime_for(&hq, &clock, t.ada);
    assert!(ada.told.contains("challenge"), "{}", ada.told);
    let _ = (t.cy, t.dee);
}

/// The owner, 2026-10-25: no game of chess had ever been played in the office, since only a
/// downtime turn could start one and members with work never have downtime. Two free members
/// are now set a game when none is being played, and its moves are picked up as before.
#[test]
fn two_free_members_are_set_a_game_of_chess_when_none_is_being_played() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, true);
    let t = team(&hq);
    only(&hq, &t, [t.bo, t.cy]);
    hq.tick().unwrap();
    let games = hq.store().chess_games().unwrap();
    assert_eq!(games.len(), 1, "one game");
    let game = &games[0];
    let players = [game.white, game.black];
    assert!(
        players.contains(&t.bo) && players.contains(&t.cy),
        "{players:?}"
    );
    assert_eq!(events(&hq, "chess.started").len(), 1);
    // White is given a turn for its move.
    assert!(
        hq.wakes()
            .unwrap()
            .iter()
            .any(|w| w.member == game.white && w.reason == WakeReason::Downtime),
        "white is woken to move"
    );
    // Not a second game while one is being played.
    end_all(&hq);
    clock.0.fetch_add(3 * 60 * 60 * 1000, Ordering::SeqCst);
    hq.tick().unwrap();
    assert_eq!(hq.store().chess_games().unwrap().len(), 1);
}

#[test]
fn nobody_is_set_chess_who_holds_work_and_one_free_member_plays_nobody() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock, false, true);
    let t = team(&hq);
    only(&hq, &t, [t.bo, t.cy]);
    let goal = hq.set_goal("Keep the lab", "", Author::Owner).unwrap();
    hq.add_task(
        Author::Owner,
        crate::application::ports::NewTask {
            goal,
            project: None,
            parent: None,
            title: "Look at sonarr".into(),
            description: String::new(),
            assignee: Some(t.cy),
            priority: crate::domain::Priority::Normal,
            waits_on: vec![],
        },
    )
    .unwrap();
    hq.tick().unwrap();
    assert!(hq.store().chess_games().unwrap().is_empty());
}
