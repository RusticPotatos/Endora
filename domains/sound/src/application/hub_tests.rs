use std::sync::Mutex;

use super::hub::{Hub, HubError};
use super::ports::{SoundStore, StoreError};
use crate::domain::{Level, Patchbay, Refusal, SpeakerKind};

/// A store in memory that counts its saves, and can be made to fail.
#[derive(Default)]
struct Kept {
    bay: Mutex<Option<Patchbay>>,
    saves: Mutex<usize>,
    failing: Mutex<bool>,
}

impl SoundStore for &Kept {
    fn load(&self) -> Result<Option<Patchbay>, StoreError> {
        Ok(self.bay.lock().unwrap().clone())
    }

    fn save(&self, bay: &Patchbay) -> Result<(), StoreError> {
        if *self.failing.lock().unwrap() {
            return Err(StoreError("the disk is full".to_owned()));
        }
        *self.saves.lock().unwrap() += 1;
        *self.bay.lock().unwrap() = Some(bay.clone());
        Ok(())
    }
}

#[test]
fn a_change_is_kept_and_a_reopened_hub_has_it() {
    let kept = Kept::default();
    let hub = Hub::open(&kept).unwrap();
    hub.change(|bay| bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None))
        .unwrap();
    let again = Hub::open(&kept).unwrap();
    assert_eq!(again.read(|bay| bay.speakers().len()), 1);
}

#[test]
fn a_refused_change_is_not_kept() {
    let kept = Kept::default();
    let hub = Hub::open(&kept).unwrap();
    let refused = hub.change(|bay| bay.add_speaker("  ", SpeakerKind::AirPlay, None));
    assert_eq!(refused, Err(HubError::Refused(Refusal::BadName)));
    assert_eq!(*kept.saves.lock().unwrap(), 0);
}

#[test]
fn a_change_that_cannot_be_kept_is_not_made() {
    let kept = Kept::default();
    let hub = Hub::open(&kept).unwrap();
    let kitchen = hub
        .change(|bay| bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None))
        .unwrap();
    *kept.failing.lock().unwrap() = true;
    let failed = hub.change(|bay| bay.set_volume(kitchen, Level::new(10).unwrap()));
    assert!(matches!(failed, Err(HubError::Store(_))));
    assert_eq!(
        hub.read(|bay| bay.speaker(kitchen).unwrap().volume),
        Level::FULL
    );
}

#[test]
fn a_change_that_changes_nothing_is_not_kept() {
    let kept = Kept::default();
    let hub = Hub::open(&kept).unwrap();
    let kitchen = hub
        .change(|bay| bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None))
        .unwrap();
    let saves = *kept.saves.lock().unwrap();
    // Taking off a wire that is not there.
    let radio = crate::domain::SourceId(99);
    assert!(!hub.change(|bay| Ok(bay.unwire(radio, kitchen))).unwrap());
    assert_eq!(*kept.saves.lock().unwrap(), saves);
}
