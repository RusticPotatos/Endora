//! Scheduling infrastructure — SQLite-backed schedule repositories.

use endora_kernel::RepositoryError;
use endora_kernel::ids::Timestamp;
use endora_persistence::{Db, backend};
use rusqlite::{OptionalExtension, params};

use crate::application::{
    BriefScheduleRepository, CheckinRepository, NightlyLoopScheduleRepository,
};
use crate::domain::{BriefSchedule, CheckinSchedule, NightlyLoopSchedule};

/// Creates the scheduling tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS checkin (
                id          INTEGER PRIMARY KEY CHECK (id = 0),
                enabled     INTEGER NOT NULL,
                interval_ms INTEGER NOT NULL,
                next_at_ms  INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS brief_schedule (
                id        INTEGER PRIMARY KEY CHECK (id = 0),
                enabled   INTEGER NOT NULL,
                hour_utc  INTEGER NOT NULL,
                last_ms   INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE IF NOT EXISTS night_loop_schedule (
                id        INTEGER PRIMARY KEY CHECK (id = 0),
                enabled   INTEGER NOT NULL,
                hour_utc  INTEGER NOT NULL,
                last_ms   INTEGER NOT NULL
            ) STRICT;",
        )
        .map_err(backend)?;
    Ok(())
}

/// SQLite-backed store for both schedules (each a single-row config table).
pub struct ScheduleStore {
    db: Db,
}

impl ScheduleStore {
    /// Builds a schedule store over the shared connection handle.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl CheckinRepository for ScheduleStore {
    fn get(&self) -> Result<Option<CheckinSchedule>, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT enabled, interval_ms, next_at_ms FROM checkin WHERE id = 0",
                [],
                |r| {
                    Ok(CheckinSchedule {
                        enabled: r.get::<_, i64>(0)? != 0,
                        interval_ms: r.get::<_, i64>(1)?,
                        next_at: Timestamp::from_unix_millis(r.get::<_, i64>(2)?),
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn set(&self, schedule: &CheckinSchedule) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO checkin (id, enabled, interval_ms, next_at_ms) \
                 VALUES (0, ?1, ?2, ?3)",
                params![
                    i64::from(schedule.enabled),
                    schedule.interval_ms,
                    schedule.next_at.unix_millis()
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl BriefScheduleRepository for ScheduleStore {
    fn get(&self) -> Result<Option<BriefSchedule>, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT enabled, hour_utc, last_ms FROM brief_schedule WHERE id = 0",
                [],
                |r| {
                    Ok(BriefSchedule {
                        enabled: r.get::<_, i64>(0)? != 0,
                        hour_utc: r.get::<_, i64>(1)? as u8,
                        last_at: Timestamp::from_unix_millis(r.get::<_, i64>(2)?),
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn set(&self, schedule: &BriefSchedule) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO brief_schedule (id, enabled, hour_utc, last_ms) \
                 VALUES (0, ?1, ?2, ?3)",
                params![
                    i64::from(schedule.enabled),
                    i64::from(schedule.hour_utc),
                    schedule.last_at.unix_millis()
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}

impl NightlyLoopScheduleRepository for ScheduleStore {
    fn get(&self) -> Result<Option<NightlyLoopSchedule>, RepositoryError> {
        self.db
            .lock()?
            .query_row(
                "SELECT enabled, hour_utc, last_ms FROM night_loop_schedule WHERE id = 0",
                [],
                |r| {
                    Ok(NightlyLoopSchedule {
                        enabled: r.get::<_, i64>(0)? != 0,
                        hour_utc: r.get::<_, i64>(1)? as u8,
                        last_at: Timestamp::from_unix_millis(r.get::<_, i64>(2)?),
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn set(&self, schedule: &NightlyLoopSchedule) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT OR REPLACE INTO night_loop_schedule (id, enabled, hour_utc, last_ms) \
                 VALUES (0, ?1, ?2, ?3)",
                params![
                    i64::from(schedule.enabled),
                    i64::from(schedule.hour_utc),
                    schedule.last_at.unix_millis()
                ],
            )
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ScheduleStore, migrate};
    use crate::application::{
        BriefScheduleRepository, CheckinRepository, NightlyLoopScheduleRepository,
    };
    use crate::domain::{BriefSchedule, CheckinSchedule, NightlyLoopSchedule};
    use endora_kernel::ids::Timestamp;
    use endora_persistence::Db;

    #[test]
    fn schedules_round_trip_or_are_absent() {
        let db = Db::open_in_memory().unwrap();
        migrate(&db).unwrap();
        let store = ScheduleStore::new(db);
        assert!(CheckinRepository::get(&store).unwrap().is_none());
        assert!(BriefScheduleRepository::get(&store).unwrap().is_none());

        let checkin = CheckinSchedule::disabled_default(Timestamp::from_unix_millis(0));
        CheckinRepository::set(&store, &checkin).unwrap();
        assert_eq!(CheckinRepository::get(&store).unwrap(), Some(checkin));

        let brief = BriefSchedule {
            enabled: true,
            hour_utc: 9,
            last_at: Timestamp::from_unix_millis(42),
        };
        BriefScheduleRepository::set(&store, &brief).unwrap();
        assert_eq!(BriefScheduleRepository::get(&store).unwrap(), Some(brief));

        assert!(
            NightlyLoopScheduleRepository::get(&store)
                .unwrap()
                .is_none()
        );
        let night = NightlyLoopSchedule {
            enabled: true,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(99),
        };
        NightlyLoopScheduleRepository::set(&store, &night).unwrap();
        assert_eq!(
            NightlyLoopScheduleRepository::get(&store).unwrap(),
            Some(night)
        );
    }

    #[test]
    fn nightly_loop_is_due_only_on_its_hour_once_per_night() {
        let hour_ms = 3_600_000_i64;
        let at = |h: i64| Timestamp::from_unix_millis(h * hour_ms);
        // last_at = 0; hour 27 is UTC hour 3 (27 % 24) a full day later, so the
        // ~20h once-per-night guard is satisfied.
        let sched = NightlyLoopSchedule {
            enabled: true,
            hour_utc: 3,
            last_at: Timestamp::from_unix_millis(0),
        };
        assert!(
            sched.is_due(at(27)),
            "due at its UTC hour after a day's gap"
        );
        assert!(!sched.is_due(at(28)), "not due off its hour (28 % 24 = 4)");
        // Just ran this night: not due again on the same hour.
        let ran = NightlyLoopSchedule {
            last_at: at(27),
            ..sched
        };
        assert!(!ran.is_due(at(27)));
        // Off is never due.
        let off = NightlyLoopSchedule {
            enabled: false,
            ..sched
        };
        assert!(!off.is_due(at(27)));
    }
}
