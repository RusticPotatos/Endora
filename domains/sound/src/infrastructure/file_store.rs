//! The patchbay kept in one JSON file, replaced whole on every save.

use std::io::Write as _;
use std::path::PathBuf;

use crate::application::ports::{SoundStore, StoreError};
use crate::domain::Patchbay;
use crate::infrastructure::json;

/// A JSON file holding the patchbay. A save writes a new file beside it and renames it over
/// the old one, so a crash part way through leaves the last good one in place.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// The store at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn beside(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".saving");
        self.path.with_file_name(name)
    }
}

impl SoundStore for FileStore {
    fn load(&self) -> Result<Option<Patchbay>, StoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StoreError(format!("{}: {e}", self.path.display()))),
        };
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| StoreError(format!("{} is not JSON: {e}", self.path.display())))?;
        json::from_json(&value)
            .map(Some)
            .map_err(|e| StoreError(format!("{}: {e}", self.path.display())))
    }

    fn save(&self, bay: &Patchbay) -> Result<(), StoreError> {
        let failed = |e: std::io::Error| StoreError(format!("{}: {e}", self.path.display()));
        let text = serde_json::to_string_pretty(&json::to_json(bay))
            .map_err(|e| StoreError(e.to_string()))?;
        let beside = self.beside();
        let mut file = std::fs::File::create(&beside).map_err(failed)?;
        file.write_all(text.as_bytes()).map_err(failed)?;
        file.sync_all().map_err(failed)?;
        std::fs::rename(&beside, &self.path).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Level, SourceKind, SpeakerKind};

    fn temporary(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "endora-sound-test-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("sound.json")
    }

    fn level(n: u8) -> Level {
        Level::new(n).unwrap()
    }

    #[test]
    fn nothing_kept_yet_is_nothing() {
        assert_eq!(FileStore::new(temporary("empty")).load(), Ok(None));
    }

    #[test]
    fn everything_comes_back_as_it_was() {
        let path = temporary("round");
        let mut bay = Patchbay::default();
        let phone = bay
            .add_source("Endora · iPhone", SourceKind::AirPlay, Some("iphone"))
            .unwrap();
        let radio = bay
            .add_source(
                "Classical",
                SourceKind::Radio("https://radio.example.com/c".to_owned()),
                None,
            )
            .unwrap();
        bay.add_source("Endora", SourceKind::Voice, None).unwrap();
        let kitchen = bay
            .add_speaker("Kitchen", SpeakerKind::AirPlay, None)
            .unwrap();
        let office = bay
            .add_speaker("Office", SpeakerKind::AirPlay, None)
            .unwrap();
        bay.add_speaker("iPhone", SpeakerKind::Stream, Some("iphone"))
            .unwrap();
        bay.wire(radio, kitchen, level(60)).unwrap();
        bay.wire(phone, office, level(90)).unwrap();
        bay.set_volume(office, level(40)).unwrap();
        bay.set_borrowed(kitchen, true).unwrap();
        bay.save_setup("Kitchen mornings").unwrap();
        let store = FileStore::new(&path);
        store.save(&bay).unwrap();
        assert_eq!(store.load().unwrap(), Some(bay));
        assert!(
            !store.beside().exists(),
            "the file beside it was left behind"
        );
    }

    #[test]
    fn an_id_is_never_handed_out_twice_across_a_restart() {
        let path = temporary("ids");
        let mut bay = Patchbay::default();
        bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None)
            .unwrap();
        let gone = bay.add_source("Old", SourceKind::Spotify, None).unwrap();
        bay.remove_source(gone);
        let store = FileStore::new(&path);
        store.save(&bay).unwrap();
        let mut again = store.load().unwrap().unwrap();
        let next = again.add_source("New", SourceKind::Spotify, None).unwrap();
        assert!(next > gone, "{next} reuses or precedes {gone}");
    }

    #[test]
    fn a_file_that_is_not_a_patchbay_stops_the_hub_rather_than_starting_it_empty() {
        let path = temporary("bad");
        for bad in [
            "not json",
            r#"{"version": 2}"#,
            r#"{"version": 1, "sources": []}"#,
        ] {
            std::fs::write(&path, bad).unwrap();
            assert!(FileStore::new(&path).load().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_is_not_taken_for_none_kept_yet() {
        // A directory where the file should be: there, but not readable as one.
        let path = temporary("unreadable");
        std::fs::create_dir_all(&path).unwrap();
        assert!(FileStore::new(&path).load().is_err());
    }

    #[test]
    fn a_hand_edited_station_that_is_not_a_web_address_is_refused() {
        let path = temporary("edited");
        let mut bay = Patchbay::default();
        bay.add_source(
            "Classical",
            SourceKind::Radio("https://radio.example.com/c".to_owned()),
            None,
        )
        .unwrap();
        let store = FileStore::new(&path);
        store.save(&bay).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            text.replace("https://radio.example.com/c", "file:///etc/passwd"),
        )
        .unwrap();
        assert!(store.load().is_err());
    }
}
