//! Elastic staffing (ADR 0118): an idle member offered for furlough and never fired, a lead's
//! backlog bringing a furloughed member back before anyone is hired, the owner's own two
//! buttons, and what a furloughed member is left out of.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Engine, Settings};
use super::ports::{Clock, NewTask, TeamStore};
use crate::domain::{GoalId, Member, MemberId, Priority, ProjectId, Refusal, Role, TaskId};
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

fn engine(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(6),
        clock,
        Settings::default(),
    )
}

/// The team as the owner has it: the CEO, Ada the lead, Dee the tester, and the wing seeded
/// (Kit leads, Rae and Zed are developers).
struct Team {
    ceo: MemberId,
    dee: MemberId,
    rae: MemberId,
    goal: GoalId,
    project: ProjectId,
}

fn team(hq: &Hq<'_>) -> Team {
    let ceo = {
        let id = hq.hire("Mara", Role::Ceo, 35, 1, 3).unwrap();
        hq.set_brain(id, "claude-opus-5-5").unwrap();
        id
    };
    hq.hire("Ada", Role::Lead, 25, 2, 0).unwrap();
    let dee = hq.hire("Dee", Role::Reviewer, 20, 2, 1).unwrap();
    hq.set_goal("Keep the home lab healthy", "", Author::Owner)
        .unwrap();
    let seeded = hq.seed_wing().unwrap().expect("the wing is seeded");
    // Kit's first turn, on the plan, is over: the Mac takes one turn at a time (ADR 0131), and
    // these tests are about the staffing of the others.
    hq.tell_mac(true, "ready");
    hq.tick().unwrap();
    for started in hq.runner().started() {
        hq.runner().finish(started.run, true);
    }
    hq.tick().unwrap();
    Team {
        ceo,
        dee,
        rae: seeded.developers[0],
        goal: seeded.goal,
        project: seeded.project,
    }
}

fn member(hq: &Hq<'_>, id: MemberId) -> Member {
    hq.store()
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// An open task of the wing's project, handed to nobody (so nobody is woken).
fn open_task(hq: &Hq<'_>, t: &Team, title: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: Some(t.project),
            parent: None,
            title: title.to_owned(),
            description: String::new(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

#[test]
fn the_owner_furloughs_and_recalls_any_member_but_the_ceo() {
    let clock = Hands(AtomicI64::new(START));
    let hq = engine(&clock);
    let t = team(&hq);
    assert_eq!(
        hq.furlough(t.ceo).unwrap_err().to_string(),
        super::engine::EngineError::Refused(Refusal::IsTheCeo).to_string()
    );
    hq.furlough(t.dee).unwrap();
    assert!(member(&hq, t.dee).furloughed());
    assert!(hq.furlough(t.dee).is_err(), "already furloughed");
    assert!(hq.recall(t.rae).is_err(), "not furloughed");
    hq.recall(t.dee).unwrap();
    let dee = member(&hq, t.dee);
    assert!(!dee.furloughed());
    assert_eq!(dee.recalled_at, Some(START));
    // Open work is handed on first: the count is said.
    open_task(&hq, &t, "Tidy the roles");
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: None,
            parent: None,
            title: "Look at the proxy".to_owned(),
            description: String::new(),
            assignee: Some(t.rae),
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    let refused = hq.furlough(t.rae).unwrap_err().to_string();
    assert!(refused.contains("1 open task"), "{refused}");
}

#[test]
fn no_engine_method_or_query_ever_deletes_a_member() {
    let store = include_str!("../infrastructure/sqlite.rs").to_ascii_lowercase();
    let marker = ["delete", "from", "members"].join(" ");
    assert!(!store.contains(&marker), "the store deletes members");
    let ports = include_str!("ports.rs").to_ascii_lowercase();
    for word in ["fn delete_member", "fn remove_member", "fn fire_member"] {
        assert!(!ports.contains(word), "{word}");
    }
}
