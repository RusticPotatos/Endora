//! Deterministic ownership contract for durable goals; no network or model.
use endora_application::{Goal, GoalRecord, GoalRepository, GoalStatus, MemoryStore, Timestamp};
use endora_infrastructure::SqliteStore;

fn at(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms)
}

fn create(store: &SqliteStore) -> GoalRecord {
    let record =
        GoalRecord::new("ownership-test", Goal::new("Read a source").unwrap(), at(1)).unwrap();
    assert!(store.create_goal(&record).unwrap());
    record
}

#[test]
fn declared_criteria_cannot_change_on_retry_or_checkpoint() {
    use endora_application::goal_request::GoalCriteria;
    let store = SqliteStore::open_in_memory().unwrap();
    let goal = |count| {
        Goal::for_request(
            "Find forthcoming Panthers fixtures",
            GoalCriteria::Schedule {
                team: "Panthers".to_owned(),
                count,
            },
        )
        .unwrap()
    };
    let original = GoalRecord::new("immutable-criteria", goal(2), at(1)).unwrap();
    store.create_goal(&original).unwrap();
    let changed = original.with_goal(goal(3), at(2));
    assert!(store.create_goal(&changed).is_err());
    let lease = store
        .claim_goal(original.request_key(), at(1000))
        .unwrap()
        .unwrap();
    assert!(!store.checkpoint_goal(&changed, &lease, at(1001)).unwrap());
    assert_eq!(
        store
            .get_goal(original.request_key())
            .unwrap()
            .unwrap()
            .goal()
            .criteria(),
        original.goal().criteria()
    );
}

#[test]
fn queue_capacity_is_checked_when_creating_not_only_by_the_chat_caller() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = |n| {
        GoalRecord::new(
            &format!("queued-{n}"),
            Goal::new("Read a source").unwrap(),
            at(1),
        )
        .unwrap()
    };
    for n in 0..3 {
        assert!(store.create_goal(&record(n)).unwrap());
    }
    assert!(!store.create_goal(&record(0)).unwrap());
    assert!(store.create_goal(&record(3)).is_err());
    store.cancel_goal("queued-0", "Cancelled", at(2)).unwrap();
    assert!(store.create_goal(&record(3)).unwrap());
}

#[test]
fn only_the_current_unexpired_owner_can_checkpoint() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let old = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    assert!(
        store
            .claim_goal(record.request_key(), at(2000))
            .unwrap()
            .is_none()
    );
    assert!(!store.checkpoint_goal(&record, &old, at(31000)).unwrap());
    let new = store
        .claim_goal(record.request_key(), at(31000))
        .unwrap()
        .unwrap();
    let mut goal = record.goal().clone();
    goal.commit("read", "current evidence");
    let updated = record.with_goal(goal, at(32000));
    assert!(store.checkpoint_goal(&updated, &new, at(32000)).unwrap());
    assert!(!store.checkpoint_goal(&record, &old, at(33000)).unwrap());
    assert_eq!(
        store
            .get_goal(record.request_key())
            .unwrap()
            .unwrap()
            .goal(),
        updated.goal()
    );
}

#[test]
fn renewal_extends_ownership_and_old_release_cannot_unlock_a_replacement() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let old = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    assert!(store.renew_goal(&old, at(20000)).unwrap());
    assert!(
        store
            .claim_goal(record.request_key(), at(31000))
            .unwrap()
            .is_none()
    );
    let new = store
        .claim_goal(record.request_key(), at(50000))
        .unwrap()
        .unwrap();
    store.release_goal(&old).unwrap();
    assert!(!store.renew_goal(&old, at(51000)).unwrap());
    assert!(store.goal_owned(&new, at(51000)).unwrap());
    assert!(!store.renew_goal(&new, at(80000)).unwrap());
}

#[test]
fn cancellation_revokes_dispatch_and_late_writes() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    assert!(
        store
            .cancel_goal(record.request_key(), "user stopped it", at(2000))
            .unwrap()
    );
    assert!(!store.goal_owned(&lease, at(2001)).unwrap());
    assert!(!store.renew_goal(&lease, at(2001)).unwrap());
    assert!(!store.checkpoint_goal(&record, &lease, at(2001)).unwrap());
}

#[test]
fn a_deleted_goal_cannot_be_updated_even_if_the_key_is_created_again() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let stale = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    store.purge().unwrap();
    assert!(!store.checkpoint_goal(&record, &stale, at(2000)).unwrap());
    assert!(store.get_goal(record.request_key()).unwrap().is_none());
    create(&store);
    let current = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    assert_ne!(
        stale, current,
        "tokens must not repeat after purge or equal claim timestamps"
    );
    assert!(!store.checkpoint_goal(&record, &stale, at(2000)).unwrap());
    assert!(store.goal_owned(&current, at(2000)).unwrap());
}

#[test]
fn exhaustion_waits_for_the_last_owner_then_records_a_terminal_reason() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let mut last = None;
    for n in 0..8 {
        last = store
            .claim_goal(record.request_key(), at(1000 + n * 30000))
            .unwrap();
        assert!(last.is_some());
    }
    assert!(
        store
            .claim_goal(record.request_key(), at(211001))
            .unwrap()
            .is_none()
    );
    assert!(store.goal_owned(&last.unwrap(), at(211001)).unwrap());
    assert!(
        store
            .claim_goal(record.request_key(), at(241000))
            .unwrap()
            .is_none()
    );
    let saved = store.get_goal(record.request_key()).unwrap().unwrap();
    assert_eq!(saved.goal().status(), GoalStatus::Blocked);
    assert!(saved.goal().reason().contains("budget"));
}

#[test]
fn a_duplicate_create_does_not_overwrite_the_checkpoint() {
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    let mut goal = record.goal().clone();
    goal.commit("read", "saved observation");
    assert!(
        store
            .checkpoint_goal(&record.with_goal(goal, at(2000)), &lease, at(2000))
            .unwrap()
    );
    assert!(!store.create_goal(&record).unwrap());
    assert_eq!(
        store
            .get_goal(record.request_key())
            .unwrap()
            .unwrap()
            .goal()
            .evidence()
            .len(),
        1
    );
}

#[test]
fn two_connections_cannot_both_claim_the_same_goal() {
    let path = std::env::temp_dir().join(format!("endora-claim-{}.sqlite", uuid::Uuid::new_v4()));
    let first = SqliteStore::open(path.to_str().unwrap()).unwrap();
    create(&first);
    let second = SqliteStore::open(path.to_str().unwrap()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = [first, second]
        .into_iter()
        .map(|store| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.claim_goal("ownership-test", at(1000)).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let claims = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn restart_preserves_the_claim_and_reclaims_only_after_expiry() {
    let path = std::env::temp_dir().join(format!("endora-reclaim-{}.sqlite", uuid::Uuid::new_v4()));
    let stale;
    {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        let record = create(&store);
        stale = store
            .claim_goal(record.request_key(), at(1000))
            .unwrap()
            .unwrap();
        let mut goal = record.goal().clone();
        goal.commit("read", "persisted observation");
        store
            .checkpoint_goal(&record.with_goal(goal, at(2000)), &stale, at(2000))
            .unwrap();
    }
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert!(
        store
            .claim_goal("ownership-test", at(3000))
            .unwrap()
            .is_none()
    );
    let current = store
        .claim_goal("ownership-test", at(31000))
        .unwrap()
        .unwrap();
    assert_ne!(current, stale);
    assert!(!store.goal_owned(&stale, at(32000)).unwrap());
    assert!(store.goal_owned(&current, at(32000)).unwrap());
    assert_eq!(
        store
            .get_goal("ownership-test")
            .unwrap()
            .unwrap()
            .goal()
            .evidence()[0]
            .output,
        "persisted observation"
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_older_lease_schema_migrates_without_resetting_attempts() {
    let path = std::env::temp_dir().join(format!(
        "endora-lease-migration-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    {
        let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
        create(&store);
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("ALTER TABLE execution_leases DROP COLUMN owner_token", [])
            .unwrap();
        conn.execute("INSERT INTO execution_leases (execution_key, attempts, lease_until_ms) VALUES ('goal:ownership-test',7,10000)", []).unwrap();
    }
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert!(
        store
            .claim_goal("ownership-test", at(9000))
            .unwrap()
            .is_none()
    );
    let lease = store
        .claim_goal("ownership-test", at(10000))
        .unwrap()
        .unwrap();
    store.release_goal(&lease).unwrap();
    assert!(
        store
            .claim_goal("ownership-test", at(10001))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_goal("ownership-test")
            .unwrap()
            .unwrap()
            .goal()
            .status(),
        GoalStatus::Blocked
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_dispatch_guard_preserves_policy_and_stops_after_cancellation() {
    use endora_application::{CapabilityRunner, CapabilitySpec, Clock};
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            at(2000)
        }
    }
    struct Runner(std::cell::Cell<usize>);
    impl CapabilityRunner for Runner {
        fn available(&self) -> Vec<CapabilitySpec> {
            Vec::new()
        }
        fn decision(&self, _: &str) -> Option<endora_application::Decision> {
            Some(endora_application::Decision::Block)
        }
        fn run(&self, _: &str, _: &str) -> Result<String, String> {
            self.0.set(self.0.get() + 1);
            Ok("observed".to_owned())
        }
    }
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    let inner = Runner(std::cell::Cell::new(0));
    let runner = endora_application::goal_execution::OwnedGoalRunner::new(
        &inner,
        &store,
        &lease,
        &FixedClock,
    );
    assert_eq!(
        runner.decision("read"),
        Some(endora_application::Decision::Block)
    );
    assert!(runner.run("read", "{}").is_ok());
    store
        .cancel_goal(record.request_key(), "stopped", at(2000))
        .unwrap();
    assert!(runner.run("read", "{}").is_err());
    assert_eq!(
        inner.0.get(),
        1,
        "no second tool dispatch after cancellation"
    );
}

#[test]
fn legacy_lease_operations_cannot_release_or_revive_a_goal_owner() {
    use endora_application::ExecutionLeaseRepository;
    let store = SqliteStore::open_in_memory().unwrap();
    let record = create(&store);
    let lease = store
        .claim_goal(record.request_key(), at(1000))
        .unwrap()
        .unwrap();
    store
        .release_execution_attempt("goal:ownership-test", 31000)
        .unwrap();
    assert!(store.goal_owned(&lease, at(2000)).unwrap());
    assert!(
        store
            .reserve_execution_attempt("goal:ownership-test", at(31000), 8)
            .unwrap()
    );
    assert!(!store.goal_owned(&lease, at(32000)).unwrap());
    assert!(!store.checkpoint_goal(&record, &lease, at(32000)).unwrap());
}
