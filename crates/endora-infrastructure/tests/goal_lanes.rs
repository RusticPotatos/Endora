//! Two lanes of goals (ADR 0094): the person's, and the ones Endora starts for itself. A goal's
//! origin decides how many may be open, and who is claimed first. Deterministic: no network,
//! no model.
use endora_application::{
    Goal, GoalOrigin, GoalRecord, GoalRepository, GoalStatus, GoalVerdict, MemoryStore, Timestamp,
};
use endora_infrastructure::SqliteStore;

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn goal(key: &str, origin: GoalOrigin, created: i64) -> GoalRecord {
    GoalRecord::new(key, Goal::new("Read a source").unwrap(), at(created))
        .unwrap()
        .with_origin(origin)
}

fn person(key: &str, created: i64) -> GoalRecord {
    goal(key, GoalOrigin::Person, created)
}

fn record(key: &str, created: i64) -> GoalRecord {
    goal(key, GoalOrigin::Record, created)
}

fn keys(goals: &[GoalRecord]) -> Vec<&str> {
    goals.iter().map(GoalRecord::request_key).collect()
}

#[test]
fn each_lane_is_counted_on_its_own() {
    let store = SqliteStore::open_in_memory().unwrap();
    // The person's lane full…
    for n in 0..3 {
        assert!(
            store
                .create_goal(&person(&format!("asked-{n}"), 1))
                .unwrap()
        );
    }
    assert!(
        store.create_goal(&person("asked-3", 1)).is_err(),
        "a fourth request is still refused: the person's cap is unchanged"
    );
    // …must not stop Endora's own lane having its one…
    assert!(
        store.create_goal(&record("looked-into-it", 1)).unwrap(),
        "a full person lane crowded out the record lane"
    );
    // …and the record lane holds one and no more.
    assert!(store.create_goal(&record("looked-into-it-too", 1)).is_err());
}

#[test]
fn a_record_goal_cannot_use_up_the_persons_slots() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert!(store.create_goal(&record("looked-into-it", 1)).unwrap());
    // The person still gets all three, however busy Endora is with its own.
    for n in 0..3 {
        assert!(
            store
                .create_goal(&person(&format!("asked-{n}"), 2))
                .unwrap(),
            "request {n} was refused because of a goal Endora started itself"
        );
    }
}

#[test]
fn ending_a_goal_frees_only_its_own_lane() {
    let store = SqliteStore::open_in_memory().unwrap();
    for n in 0..3 {
        store
            .create_goal(&person(&format!("asked-{n}"), 1))
            .unwrap();
    }
    store.create_goal(&record("looked-into-it", 1)).unwrap();

    store
        .cancel_goal("looked-into-it", "stopped", at(2))
        .unwrap();
    assert!(
        store.create_goal(&record("another", 3)).unwrap(),
        "the record lane is free again"
    );
    assert!(
        store.create_goal(&person("asked-3", 3)).is_err(),
        "and the person's lane is exactly as full as it was"
    );
}

#[test]
fn a_persons_goal_is_claimed_before_an_older_record_goal() {
    let store = SqliteStore::open_in_memory().unwrap();
    // Endora's own goal is the older one, and was touched earlier. Age alone would put it
    // first; the worker takes the first claimable goal in this order.
    store.create_goal(&record("looked-into-it", 1)).unwrap();
    store.create_goal(&person("asked-later", 50)).unwrap();

    let order = store.active_goals().unwrap();
    assert_eq!(
        keys(&order),
        ["asked-later", "looked-into-it"],
        "a request waited behind work nobody asked for"
    );
}

#[test]
fn within_a_lane_the_oldest_goal_is_still_first() {
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_goal(&person("second", 20)).unwrap();
    store.create_goal(&person("first", 10)).unwrap();
    store.create_goal(&record("only-record", 5)).unwrap();
    assert_eq!(
        keys(&store.active_goals().unwrap()),
        ["first", "second", "only-record"]
    );
}

#[test]
fn origin_is_reported_by_every_way_a_goal_is_read_and_survives_a_checkpoint() {
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_goal(&record("looked-into-it", 1)).unwrap();

    // get_goal
    assert_eq!(
        store.get_goal("looked-into-it").unwrap().unwrap().origin(),
        GoalOrigin::Record
    );
    // active_goals
    assert_eq!(
        store.active_goals().unwrap()[0].origin(),
        GoalOrigin::Record
    );

    // A checkpoint is the worker's every pass. It must not touch where the goal came from.
    let lease = store
        .claim_goal("looked-into-it", at(1000))
        .unwrap()
        .unwrap();
    let mut finished = store.get_goal("looked-into-it").unwrap().unwrap();
    let mut done = finished.goal().clone();
    done.start("gathering", "a step");
    done.commit("a step", "something found");
    done.apply(GoalVerdict::Complete, "verified");
    finished = finished.with_goal(done, at(1001));
    assert_eq!(finished.origin(), GoalOrigin::Record);
    assert!(store.checkpoint_goal(&finished, &lease, at(1001)).unwrap());

    let after = store.get_goal("looked-into-it").unwrap().unwrap();
    assert_eq!(after.goal().status(), GoalStatus::Complete);
    assert_eq!(
        after.origin(),
        GoalOrigin::Record,
        "a checkpoint changed the lane"
    );

    // pending_goal_completions
    let pending = store.pending_goal_completions().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].origin(), GoalOrigin::Record);
    // the export
    let exported = store.export().unwrap().goals;
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].origin(), GoalOrigin::Record);
}

#[test]
fn a_goal_key_cannot_be_reused_across_lanes() {
    // A retry of the same request must be the same request. Reusing a key with a different
    // origin would let a self-started goal be swapped for a person's, or the reverse.
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_goal(&person("shared-key", 1)).unwrap();
    assert!(store.create_goal(&record("shared-key", 2)).is_err());
    assert!(
        !store.create_goal(&person("shared-key", 3)).unwrap(),
        "the same request, retried, is still just a duplicate"
    );
}

#[test]
fn a_database_from_before_origin_existed_reads_every_goal_as_the_persons() {
    let path = std::env::temp_dir().join(format!(
        "endora-origin-migration-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        store.create_goal(&person("asked-before", 1)).unwrap();
    }
    {
        // Put it back the way it was before this change: no origin column at all.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("ALTER TABLE execution_goals DROP COLUMN origin", [])
            .unwrap();
    }
    // Opening migrates it. Twice, because a migration that is not idempotent fails on the
    // second restart rather than the first.
    for _ in 0..2 {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        let old = store.get_goal("asked-before").unwrap().unwrap();
        assert_eq!(
            old.origin(),
            GoalOrigin::Person,
            "an older goal changed lane"
        );
        assert_eq!(old.goal().status(), GoalStatus::Planned);
    }
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert!(
        store.create_goal(&record("looked-into-it", 2)).unwrap(),
        "the migrated database accepts a record-lane goal"
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn an_unreadable_stored_origin_is_a_records_and_waits_behind_a_person() {
    let path = std::env::temp_dir().join(format!(
        "endora-origin-garbage-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        store.create_goal(&person("asked", 50)).unwrap();
        store.create_goal(&person("mystery", 1)).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE execution_goals SET origin='from-a-newer-build' WHERE request_key='mystery'",
            [],
        )
        .unwrap();
    }
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        store.get_goal("mystery").unwrap().unwrap().origin(),
        GoalOrigin::Record,
        "a value this build cannot read must not be trusted as the person's"
    );
    assert_eq!(
        keys(&store.active_goals().unwrap()),
        ["asked", "mystery"],
        "and so it never outranks a request, even though it is older"
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn from_a_trouble(key: &str, origin: GoalOrigin, created: i64) -> GoalRecord {
    use endora_application::goal_request::{GoalCriteria, WorkSource};
    let goal = Goal::for_request(
        "sensor.garage_door has not answered for 4 days — find out why",
        GoalCriteria::Resolve {
            source: WorkSource::Trouble {
                server: "home".to_owned(),
                thing: "sensor.garage_door".to_owned(),
                since_ms: 1,
            },
        },
    )
    .unwrap();
    GoalRecord::new(key, goal, at(created))
        .unwrap()
        .with_origin(origin)
}

#[test]
fn a_goal_that_resolves_from_a_record_reads_back_whole() {
    let store = SqliteStore::open_in_memory().unwrap();
    let own = from_a_trouble(
        "own:trouble:home:sensor.garage_door:1:1",
        GoalOrigin::Record,
        5,
    );
    assert!(store.create_goal(&own).unwrap());
    let back = store.get_goal(own.request_key()).unwrap().unwrap();
    assert_eq!(back.goal().criteria(), own.goal().criteria());
    assert_eq!(back.origin(), GoalOrigin::Record);
}

#[test]
fn a_person_lane_goal_cannot_carry_a_source_only_endora_derives() {
    // "Done when this record says so" is the authority of work Endora derived itself. A goal in
    // the person's lane that carried it would be done on a record's say-so, not the person's.
    let store = SqliteStore::open_in_memory().unwrap();
    let borrowed = from_a_trouble("asked-for", GoalOrigin::Person, 5);
    assert!(store.create_goal(&borrowed).is_err());
    assert!(store.get_goal("asked-for").unwrap().is_none());
}

#[test]
fn the_goals_endora_started_are_listed_without_the_persons() {
    let store = SqliteStore::open_in_memory().unwrap();
    store.create_goal(&person("asked", 100)).unwrap();
    let old = from_a_trouble("own:old", GoalOrigin::Record, 10);
    store.create_goal(&old).unwrap();
    store.cancel_goal("own:old", "stopped", at(11)).unwrap();
    store
        .create_goal(&from_a_trouble("own:recent", GoalOrigin::Record, 200))
        .unwrap();

    let all = store.own_goals_since(at(0)).unwrap();
    assert_eq!(keys(&all), ["own:old", "own:recent"]);

    let recent = store.own_goals_since(at(100)).unwrap();
    assert_eq!(
        keys(&recent),
        ["own:recent"],
        "an ended goal older than the window is out of it"
    );
}

#[test]
fn a_goal_endora_started_is_listed_however_old_while_it_is_still_open() {
    // The lane being occupied is a fact about now, not about when the goal began.
    let store = SqliteStore::open_in_memory().unwrap();
    store
        .create_goal(&from_a_trouble("own:long-running", GoalOrigin::Record, 10))
        .unwrap();
    let listed = store.own_goals_since(at(1_000_000)).unwrap();
    assert_eq!(keys(&listed), ["own:long-running"]);
}
