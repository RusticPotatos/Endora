use super::*;

const MIN: Millis = 60_000;
const HOUR: Millis = 60 * MIN;
const NOW: Millis = 1_000 * 24 * HOUR;

fn member(name: &str) -> MemberSeen {
    MemberSeen {
        name: name.into(),
        brain: "Claude Haiku".into(),
        paused: false,
        furloughed: false,
        out_of_budget: false,
        turns: vec![],
    }
}

fn ended(run: i64, task: Option<i64>, at: Millis, downtime: bool) -> TurnSeen {
    TurnSeen {
        run,
        task,
        status: "succeeded".into(),
        started: Some(at - MIN),
        ended: Some(at),
        downtime,
        last_signal: None,
    }
}

fn item(task: i64, holder: Option<&str>, status: &str, project: &str) -> ItemSeen {
    ItemSeen {
        task,
        title: format!("Item {task}"),
        status: status.into(),
        holder: holder.map(Into::into),
        project: Some(project.into()),
    }
}

fn card(id: i64, kind: &str, task: Option<i64>, project: Option<&str>) -> CardSeen {
    CardSeen {
        id,
        kind: kind.into(),
        task,
        project: project.map(Into::into),
        at: NOW - 5 * HOUR + id,
    }
}

/// Today's shape: nobody running, Sol holding blocked items with questions, a desk waiting.
fn quiet() -> Seen {
    let mut sol = member("Sol");
    sol.turns = vec![ended(10, Some(1), NOW - 30 * HOUR, false)];
    let mut bo = member("Bo");
    bo.turns = vec![
        ended(11, Some(5), NOW - 4 * HOUR, false),
        ended(12, None, NOW - 20 * MIN, true),
    ];
    let mut cy = member("Cy");
    cy.furloughed = true;
    let mut mara = member("Mara");
    mara.out_of_budget = true;
    Seen {
        now: NOW,
        members: vec![sol, bo, cy, mara],
        items: vec![
            item(1, Some("Sol"), "blocked", "Harness climb"),
            item(2, Some("Sol"), "blocked", "Harness climb"),
            item(3, Some("Sol"), "blocked", "Set 3 solo"),
            item(4, None, "blocked", "Home lab upkeep"),
        ],
        desk: vec![
            card(100, "question", Some(1), Some("Harness climb")),
            card(101, "project_budget", None, Some("Harness climb")),
            card(102, "privacy", Some(9), None),
        ],
    }
}

#[test]
fn with_nothing_running_the_map_says_so_and_nothing_moves() {
    let map = team_map(&quiet());
    assert_eq!(map.running, 0);
    assert_eq!(map.said, "Nothing is running. 3 cards wait on you.");
    assert!(map.members.iter().all(|m| m.live.iter().all(|l| !l.moving)));
}

#[test]
fn a_running_turn_with_a_fresh_sign_is_the_only_thing_that_moves() {
    let mut seen = quiet();
    seen.members[1].turns.push(TurnSeen {
        run: 13,
        task: Some(5),
        status: "running".into(),
        started: Some(NOW - 2 * MIN),
        ended: None,
        downtime: false,
        last_signal: Some(NOW - MIN),
    });
    let map = team_map(&seen);
    assert_eq!(map.running, 1);
    assert_eq!(map.said, "1 turn running. 3 cards wait on you.");
    let bo = map.members.iter().find(|m| m.name == "Bo").unwrap();
    assert_eq!(bo.state, MemberState::Working);
    assert!(bo.live[0].moving);
    assert!(
        map.members
            .iter()
            .filter(|m| m.name != "Bo")
            .all(|m| m.live.is_empty())
    );
}

#[test]
fn a_running_turn_silent_past_the_window_is_still_and_says_since_when() {
    let mut seen = quiet();
    let since = NOW - 2 * HOUR;
    seen.members[1].turns.push(TurnSeen {
        run: 13,
        task: Some(5),
        status: "running".into(),
        started: Some(since),
        ended: None,
        downtime: false,
        last_signal: None,
    });
    let bo_map = team_map(&seen);
    let bo = bo_map.members.iter().find(|m| m.name == "Bo").unwrap();
    assert_eq!(bo.state, MemberState::Unknown);
    assert!(!bo.live[0].moving);
    assert_eq!(bo.live[0].heard, since);
}

#[test]
fn a_downtime_turn_is_not_work_but_is_when_it_was_last_seen() {
    let map = team_map(&quiet());
    let bo = map.members.iter().find(|m| m.name == "Bo").unwrap();
    assert_eq!(bo.state, MemberState::Idle);
    assert_eq!(bo.last_seen, Some(NOW - 20 * MIN));
    assert_eq!(bo.last_work, Some((NOW - 4 * HOUR, Some(5))));
}

#[test]
fn furloughed_and_out_of_budget_members_say_so() {
    let map = team_map(&quiet());
    let state = |n: &str| map.members.iter().find(|m| m.name == n).unwrap().state;
    assert_eq!(state("Cy"), MemberState::Furloughed);
    assert_eq!(state("Mara"), MemberState::OutForToday);
}

#[test]
fn items_hang_under_their_holder_grouped_by_project_with_the_owners_question_marked() {
    let map = team_map(&quiet());
    let sol = map.members.iter().find(|m| m.name == "Sol").unwrap();
    let groups: Vec<(&str, usize)> = sol
        .groups
        .iter()
        .map(|g| (g.project.as_str(), g.items.len()))
        .collect();
    assert_eq!(groups, [("Harness climb", 2), ("Set 3 solo", 1)]);
    assert!(sol.groups[0].items[0].asks_owner);
    assert!(!sol.groups[0].items[1].asks_owner);
    assert_eq!(map.blocked, 4);
}

#[test]
fn an_item_nobody_holds_is_said_apart() {
    let map = team_map(&quiet());
    assert_eq!(map.unheld.len(), 1);
    assert_eq!(map.unheld[0].task, 4);
}

#[test]
fn the_owners_desk_is_split_by_project_with_the_rest_as_other() {
    let map = team_map(&quiet());
    assert_eq!(map.desk_total, 3);
    let groups: Vec<(&str, usize)> = map
        .desk
        .iter()
        .map(|g| (g.project.as_str(), g.count))
        .collect();
    assert_eq!(groups, [("Harness climb", 2), ("other", 1)]);
    assert_eq!(map.desk_oldest, Some(NOW - 5 * HOUR + 100));
}

#[test]
fn members_with_work_come_first_then_the_idle_then_those_away() {
    let map = team_map(&quiet());
    let order: Vec<&str> = map.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(order, ["Sol", "Bo", "Mara", "Cy"]);
}
