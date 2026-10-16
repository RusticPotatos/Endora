use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Store {
    brief: RefCell<Option<BriefSchedule>>,
    night: RefCell<Option<NightlyLoopSchedule>>,
    settings: RefCell<Vec<(String, String, String)>>,
}

impl BriefScheduleRepository for Store {
    fn get(&self) -> Result<Option<BriefSchedule>, endora_kernel::RepositoryError> {
        Ok(*self.brief.borrow())
    }
    fn set(&self, schedule: &BriefSchedule) -> Result<(), endora_kernel::RepositoryError> {
        *self.brief.borrow_mut() = Some(*schedule);
        Ok(())
    }
}

impl NightlyLoopScheduleRepository for Store {
    fn get(&self) -> Result<Option<NightlyLoopSchedule>, endora_kernel::RepositoryError> {
        Ok(*self.night.borrow())
    }
    fn set(&self, schedule: &NightlyLoopSchedule) -> Result<(), endora_kernel::RepositoryError> {
        *self.night.borrow_mut() = Some(*schedule);
        Ok(())
    }
}

impl CapabilitySettingsRepository for Store {
    fn all_settings(
        &self,
    ) -> Result<Vec<(String, String, String)>, endora_kernel::RepositoryError> {
        Ok(self.settings.borrow().clone())
    }
    fn set_setting(
        &self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), endora_kernel::RepositoryError> {
        let mut all = self.settings.borrow_mut();
        all.retain(|(i, k, _)| !(i == id && k == key));
        all.push((id.to_owned(), key.to_owned(), value.to_owned()));
        Ok(())
    }
}

fn store(brief_hour: u8, night_hour: u8) -> Store {
    let s = Store::default();
    *s.brief.borrow_mut() = Some(BriefSchedule {
        enabled: true,
        hour_utc: brief_hour,
        last_at: endora_kernel::ids::Timestamp::from_unix_millis(5),
    });
    *s.night.borrow_mut() = Some(NightlyLoopSchedule {
        enabled: false,
        hour_utc: night_hour,
        last_at: endora_kernel::ids::Timestamp::from_unix_millis(7),
    });
    s
}

fn keep(s: &Store, offset: i64) -> bool {
    keep_hours_on_the_clock(s, s, s, offset).unwrap()
}

#[test]
fn the_first_look_only_notes_the_offset_and_the_clock_change_moves_both_hours() {
    // A 07:00 brief and a 03:00 nightly loop, set four hours west of UTC.
    let s = store(11, 7);
    assert!(!keep(&s, -240), "nothing to move the first time");
    assert_eq!(s.brief.borrow().unwrap().hour_utc, 11);
    assert!(!keep(&s, -240));
    // The clocks go back: five hours west.
    assert!(keep(&s, -300));
    let brief = s.brief.borrow().unwrap();
    let night = s.night.borrow().unwrap();
    assert_eq!((brief.hour_utc, night.hour_utc), (12, 8));
    // Only the hour moves.
    assert!(brief.enabled && !night.enabled);
    assert_eq!(brief.last_at.unix_millis(), 5);
    // Moved once, not on every look.
    assert!(!keep(&s, -300));
    assert_eq!(s.brief.borrow().unwrap().hour_utc, 12);
    // And back in spring.
    assert!(keep(&s, -240));
    assert_eq!(s.brief.borrow().unwrap().hour_utc, 11);
}

#[test]
fn with_no_schedules_stored_only_the_offset_is_kept() {
    let s = Store::default();
    assert!(!keep(&s, -240));
    assert!(keep(&s, -300));
    assert!(s.brief.borrow().is_none() && s.night.borrow().is_none());
}
