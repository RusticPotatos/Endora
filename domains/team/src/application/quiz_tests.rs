//! The nightly quiz on the map (ADR 0126): asked of a free member on the house's model in a
//! downtime turn, made by code from that member's own map and scored by code, with no paid turn;
//! skipped, and recorded as skipped, when no such member is free; and a page that fails night
//! after night flagged to its steward.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::engine::{Author, Engine, Settings};
use super::map_ports::MapStore;
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use super::protocol::call;
use crate::domain::map::map_block;
use crate::domain::quiz::{Graded, Kind, Quiz, Status};
use crate::domain::{GoalId, MemberId, Priority, Role, TaskId};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;
/// 19 October 2026, at 02:00 UTC: the dead of night.
const NIGHT: i64 = 1_792_324_800_000 + 14 * HOUR;

struct Team {
    ada: MemberId,
    linus: MemberId,
    dee: MemberId,
    goal: GoalId,
}

fn engine(clock: &Hands, quiz: bool) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            quiz,
            ..Settings::default()
        },
    )
}

/// Mara the CEO, and Ada, Linus and Dee, all on the house's model.
fn team(hq: &Hq<'_>) -> Team {
    hq.seat_ceo("Mara").unwrap().unwrap();
    let ada = hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let linus = hq.hire("Linus", Role::Developer, 40, 2, 4).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    for member in [linus, dee] {
        hq.set_reports_to(member, ada).unwrap();
    }
    let goal = hq
        .set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    hq.migrate_map().unwrap();
    Team {
        ada,
        linus,
        dee,
        goal,
    }
}

fn task_for(hq: &Hq<'_>, goal: GoalId, to: MemberId, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: None,
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: Some(to),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &Value) -> Result<String, String> {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    call(hq, &run, &member, tool, args)
}

/// The turn the quiz was asked in: the newest the runner started for its member.
fn quiz_turn(hq: &Hq<'_>, quiz: &Quiz) -> RunContext {
    let member = quiz.member.expect("asked of a member");
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("the member's turn started")
}

fn end_all(hq: &Hq<'_>) {
    for context in hq.runner().started() {
        hq.runner().finish(context.run, true);
    }
    hq.tick().unwrap();
}

#[test]
fn a_free_member_on_the_house_model_is_quizzed_at_night_on_its_own_map_and_code_scores_it() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    team(&hq);
    hq.tick().unwrap();
    let quizzes = hq.store().quizzes().unwrap();
    assert_eq!(quizzes.len(), 1);
    let quiz = &quizzes[0];
    assert_eq!(quiz.status, Status::Asked);
    assert!(quiz.questions.len() >= 2 && quiz.questions.len() <= 6);
    // It is a downtime turn, free: the house's model, no paid turn.
    let context = quiz_turn(&hq, quiz);
    let run = hq.store().run(context.run).unwrap().unwrap();
    assert!(hq.in_downtime(run.id).unwrap());
    assert_eq!(run.reserved, 0);
    // Its briefing carries the questions, and each is about a page on its own map.
    assert!(
        context.told.contains("quiz on your map"),
        "{}",
        context.told
    );
    let map = map_block(&context.told).expect("its map");
    for q in &quiz.questions {
        assert!(context.told.contains(&q.text), "{}", q.text);
        assert!(
            map.contains(&format!("world:{}", q.page)),
            "{} on\n{map}",
            q.page
        );
    }
    // Right answers score full marks; the score is kept, per page.
    let answers: Vec<String> = quiz
        .questions
        .iter()
        .map(|q| q.expects[0].clone())
        .collect();
    let said = use_tool(&hq, &context, "answer_quiz", &json!({ "answers": answers })).unwrap();
    let n = quiz.questions.len();
    assert!(said.contains(&format!("{n} of {n}")), "{said}");
    let kept = &hq.store().quizzes().unwrap()[0];
    assert_eq!(kept.status, Status::Answered);
    assert!(kept.graded.iter().all(|g| g.passed) && kept.graded.len() == n);
    assert_eq!(kept.answers, answers);
    // Once answered, it is answered; and it is the one quiz of the night.
    let again = use_tool(&hq, &context, "answer_quiz", &json!({ "answers": answers })).unwrap_err();
    assert!(again.contains("no quiz open for you"), "{again}");
    end_all(&hq);
    hq.tick().unwrap();
    assert_eq!(hq.store().quizzes().unwrap().len(), 1);
    // The next night, another.
    clock.0.fetch_add(DAY, Ordering::SeqCst);
    hq.tick().unwrap();
    let quizzes = hq.store().quizzes().unwrap();
    assert_eq!(quizzes.len(), 2);
    assert_eq!(quizzes[1].night, quizzes[0].night + 1);
}

#[test]
fn wrong_answers_fail_and_answering_fewer_than_asked_is_refused() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    team(&hq);
    hq.tick().unwrap();
    let quiz = hq.store().quizzes().unwrap().remove(0);
    let context = quiz_turn(&hq, &quiz);
    let n = quiz.questions.len();
    let short = use_tool(
        &hq,
        &context,
        "answer_quiz",
        &json!({ "answers": ["world:lab"] }),
    )
    .unwrap_err();
    assert!(
        short.contains(&format!("answer all {n} questions")),
        "{short}"
    );
    let wrong: Vec<&str> = vec!["I do not know"; n];
    let said = use_tool(&hq, &context, "answer_quiz", &json!({ "answers": wrong })).unwrap();
    assert!(said.contains(&format!("0 of {n}")), "{said}");
    let kept = &hq.store().quizzes().unwrap()[0];
    assert!(kept.graded.iter().all(|g| !g.passed), "{:?}", kept.graded);
    assert!(
        kept.graded.iter().any(|g| g.kind == Kind::WhoKeeps),
        "{:?}",
        kept.graded
    );
}

#[test]
fn a_quiz_is_only_asked_when_the_service_asks_for_it() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, false);
    team(&hq);
    hq.tick().unwrap();
    assert!(hq.store().quizzes().unwrap().is_empty());
}

#[test]
fn when_no_member_on_the_house_model_is_free_the_night_is_recorded_as_skipped() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    let t = team(&hq);
    for member in [t.ada, t.linus, t.dee] {
        let brain = match member {
            m if m == t.linus => "claude-haiku-5-5",
            _ => "claude-sonnet-5-5",
        };
        hq.set_brain(member, brain).unwrap();
    }
    // Overnight it waits for one to be free; nothing is recorded yet.
    hq.tick().unwrap();
    assert!(hq.store().quizzes().unwrap().is_empty());
    assert!(
        hq.runner().started().is_empty(),
        "no paid turn is spent on it"
    );
    // Come morning, the night is recorded as skipped, with why, once.
    clock.0.fetch_add(4 * HOUR + 30 * 60_000, Ordering::SeqCst);
    hq.tick().unwrap();
    hq.tick().unwrap();
    let quizzes = hq.store().quizzes().unwrap();
    assert_eq!(quizzes.len(), 1);
    assert_eq!(quizzes[0].status, Status::Skipped);
    assert!(quizzes[0].why.contains("free"), "{}", quizzes[0].why);
    assert!(hq.runner().started().is_empty());
}

#[test]
fn a_member_at_work_is_not_quizzed_and_the_quiz_waits_for_one_that_is_free() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    let t = team(&hq);
    task_for(&hq, t.goal, t.linus, "Fix the proxy");
    task_for(&hq, t.goal, t.ada, "Look at backups");
    task_for(&hq, t.goal, t.dee, "Test the proxy");
    hq.tick().unwrap();
    assert!(
        hq.store().quizzes().unwrap().is_empty(),
        "everyone is at work: it waits"
    );
    // Linus finishes; he is free, and he is the one asked.
    let linus = hq
        .runner()
        .started()
        .into_iter()
        .find(|c| c.member == t.linus)
        .unwrap();
    hq.runner().finish(linus.run, true);
    for task in hq.store().tasks().unwrap() {
        if task.assignee == Some(t.linus) {
            hq.store()
                .set_task_status(task.id, crate::domain::TaskStatus::Done)
                .unwrap();
        }
    }
    hq.tick().unwrap();
    let quizzes = hq.store().quizzes().unwrap();
    assert_eq!(quizzes.len(), 1, "{quizzes:?}");
    assert_eq!(quizzes[0].member, Some(t.linus));
}

#[test]
fn a_quiz_not_answered_in_twelve_hours_is_let_go_and_no_page_is_marked_for_it() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    team(&hq);
    hq.tick().unwrap();
    assert_eq!(hq.store().quizzes().unwrap()[0].status, Status::Asked);
    end_all(&hq);
    clock.0.fetch_add(13 * HOUR, Ordering::SeqCst);
    hq.tick().unwrap();
    let quiz = &hq.store().quizzes().unwrap()[0];
    assert_eq!(quiz.status, Status::Unanswered);
    assert!(quiz.graded.is_empty());
}

#[test]
fn a_page_failing_three_nights_running_is_flagged_to_its_steward_and_the_card() {
    let clock = Hands(AtomicI64::new(NIGHT));
    let hq = engine(&clock, true);
    let t = team(&hq);
    let page = format!("area/{}", t.ada.0);
    for night in 1..=3 {
        hq.store()
            .save_quiz(&Quiz {
                id: 0,
                night: 20_000 + night,
                status: Status::Answered,
                member: Some(t.linus),
                asked_at: Some(NIGHT - DAY),
                answered_at: Some(NIGHT - DAY),
                questions: Vec::new(),
                answers: Vec::new(),
                graded: vec![Graded {
                    n: 0,
                    kind: Kind::WhichPage,
                    page: page.clone(),
                    passed: false,
                }],
                why: String::new(),
            })
            .unwrap();
    }
    let shown = hq.quiz_shown().unwrap();
    let about = shown["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["slug"] == page.as_str())
        .unwrap_or_else(|| panic!("{shown}"));
    assert_eq!(about["flagged"], true);
    assert_eq!(
        (about["passed"].as_u64(), about["asked"].as_u64()),
        (Some(0), Some(3))
    );
    // Its steward is told in its next briefing; another lead is not.
    task_for(&hq, t.goal, t.ada, "Look at backups");
    hq.tick().unwrap();
    let ada = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == t.ada)
        .unwrap();
    assert!(
        ada.told.contains("failed the nightly quiz") && ada.told.contains(&format!("world:{page}")),
        "{}",
        ada.told
    );
    let linus = task_for(&hq, t.goal, t.linus, "Fix the proxy");
    hq.tick().unwrap();
    let linus_turn = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == t.linus)
        .unwrap();
    assert!(!linus_turn.told.contains("failed the nightly quiz"));
    let _ = linus;
}
