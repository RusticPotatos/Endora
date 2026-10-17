use std::sync::Mutex;

use super::*;
use endora_sound::domain::{Level, SourceKind};

/// An OwnTone instance in memory: its outputs, and everything it was told.
#[derive(Default)]
struct Fake {
    outputs: Mutex<Vec<Output>>,
    selects: Mutex<Vec<Vec<String>>>,
    volumes: Mutex<Vec<(String, u8)>>,
}

impl Instance for Arc<Fake> {
    fn outputs(&self) -> Result<Vec<Output>, String> {
        Ok(self.outputs.lock().unwrap().clone())
    }
    fn select(&self, ids: &[String]) -> Result<(), String> {
        self.selects.lock().unwrap().push(ids.to_vec());
        for o in self.outputs.lock().unwrap().iter_mut() {
            o.selected = ids.contains(&o.id);
        }
        Ok(())
    }
    fn volume(&self, id: &str, volume: u8) -> Result<(), String> {
        self.volumes.lock().unwrap().push((id.to_owned(), volume));
        for o in self
            .outputs
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|o| o.id == id)
        {
            o.volume = volume;
        }
        Ok(())
    }
}

fn output(id: &str, name: &str) -> Output {
    Output {
        id: id.to_owned(),
        name: name.to_owned(),
        selected: false,
        volume: 50,
        kind: "AirPlay".to_owned(),
    }
}

struct Setup {
    fake: Arc<Fake>,
    control: Control,
    kitchen: SpeakerId,
    office: SpeakerId,
}

fn setup() -> Setup {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("endora-sound-senders-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Arc::new(Hub::open(FileStore::new(dir.join("sound.json"))).unwrap());
    let (kitchen, office) = hub
        .change(|bay| {
            let radio = bay.add_source(
                "Classical",
                SourceKind::Radio("https://radio.example.com/c".to_owned()),
                None,
            )?;
            let k = bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None)?;
            let o = bay.add_speaker("Office", SpeakerKind::AirPlay, None)?;
            bay.wire(radio, k, Level::new(60).unwrap())?;
            bay.wire(radio, o, Level::new(60).unwrap())?;
            bay.set_volume(k, Level::new(40).unwrap())?;
            Ok((k, o))
        })
        .unwrap();
    let fake = Arc::new(Fake::default());
    *fake.outputs.lock().unwrap() = vec![
        output("111", "Kitchen"),
        output("222", "Office"),
        output("333", "Endora · iPhone"),
    ];
    let control = Control {
        hub,
        instances: vec![Box::new(fake.clone())],
        wanted: BTreeMap::new(),
        chosen: BTreeMap::new(),
    };
    Setup {
        fake,
        control,
        kitchen,
        office,
    }
}

#[test]
fn a_sender_is_told_its_speakers_by_name_and_their_volumes() {
    let mut s = setup();
    s.control.wanted.insert(0, vec![s.kitchen, s.office]);
    s.control.choose(0);
    assert_eq!(
        s.fake.selects.lock().unwrap().last().unwrap(),
        &vec!["111".to_owned(), "222".to_owned()]
    );
    let volumes = s.fake.volumes.lock().unwrap().clone();
    assert!(volumes.contains(&("111".to_owned(), 40)), "{volumes:?}");
    assert!(volumes.contains(&("222".to_owned(), 100)), "{volumes:?}");
}

#[test]
fn a_released_sender_plays_nothing() {
    let mut s = setup();
    s.control.wanted.insert(0, vec![s.kitchen]);
    s.control.choose(0);
    s.control.wanted.remove(&0);
    s.control.choose(0);
    assert_eq!(
        s.fake.selects.lock().unwrap().last().unwrap(),
        &Vec::<String>::new()
    );
}

#[test]
fn a_speaker_that_drops_out_after_playing_is_borrowed_and_let_go_at_once() {
    let mut s = setup();
    s.control.wanted.insert(0, vec![s.kitchen, s.office]);
    s.control.choose(0);
    s.control.look(0); // seen playing
    // The Mac takes the office HomePod: the instance reports it no longer selected.
    s.fake
        .outputs
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|o| o.id == "222")
        .for_each(|o| o.selected = false);
    s.control.look(0);
    assert!(
        s.control
            .hub
            .read(|bay| bay.speaker(s.office).unwrap().borrowed),
        "not marked borrowed"
    );
    assert_eq!(
        s.fake.selects.lock().unwrap().last().unwrap(),
        &vec!["111".to_owned()],
        "not let go"
    );
    assert!(
        !s.control
            .hub
            .read(|bay| bay.speaker(s.kitchen).unwrap().borrowed)
    );
}

#[test]
fn a_speaker_that_never_came_up_is_not_called_borrowed() {
    let mut s = setup();
    s.control.wanted.insert(0, vec![s.office]);
    s.control.choose(0);
    // Before it was ever seen playing, it shows unselected (still starting, or unreachable).
    s.fake
        .outputs
        .lock()
        .unwrap()
        .iter_mut()
        .for_each(|o| o.selected = false);
    s.control.look(0);
    assert!(
        !s.control
            .hub
            .read(|bay| bay.speaker(s.office).unwrap().borrowed)
    );
}

#[test]
fn a_volume_that_drifts_is_put_back() {
    let mut s = setup();
    s.control.wanted.insert(0, vec![s.kitchen]);
    s.control.choose(0);
    // Someone turns the HomePod up on the device itself.
    s.fake
        .outputs
        .lock()
        .unwrap()
        .iter_mut()
        .filter(|o| o.id == "111")
        .for_each(|o| o.volume = 90);
    s.control.look(0);
    assert_eq!(
        s.fake.volumes.lock().unwrap().last().unwrap(),
        &("111".to_owned(), 40)
    );
}

#[test]
fn the_speakers_to_add_are_the_airplay_ones_but_not_the_hubs_own_receivers() {
    let s = setup();
    s.fake.outputs.lock().unwrap().push(Output {
        kind: "Chromecast".to_owned(),
        ..output("444", "Living room TV")
    });
    assert_eq!(
        found(&s.fake).unwrap(),
        vec!["Kitchen".to_owned(), "Office".to_owned()]
    );
}

#[test]
fn an_airplay_2_speaker_is_found_too() {
    // What OwnTone really says, as the first live test showed.
    let s = setup();
    for o in s.fake.outputs.lock().unwrap().iter_mut() {
        o.kind = "AirPlay 2".to_owned();
    }
    assert_eq!(
        found(&s.fake).unwrap(),
        vec!["Kitchen".to_owned(), "Office".to_owned()]
    );
}

#[test]
fn the_speakers_names_are_written_for_the_senders_only_when_they_change() {
    let dir = std::env::temp_dir().join(format!("endora-sound-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let names = vec![
        "Office HomePod".to_owned(),
        "Kitchen".to_owned(),
        "Kitchen".to_owned(),
    ];
    assert!(
        write_speaker_names(&dir, &names).unwrap(),
        "the first time it writes"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(SPEAKER_NAMES)).unwrap(),
        "Kitchen\nOffice HomePod\n"
    );
    assert!(
        !write_speaker_names(&dir, &names).unwrap(),
        "unchanged, so the senders are not restarted"
    );
    // A name that would break the senders' list is left out.
    assert!(write_speaker_names(&dir, &["Kitchen".to_owned(), "Bad\nName".to_owned()]).unwrap());
    assert_eq!(
        std::fs::read_to_string(dir.join(SPEAKER_NAMES)).unwrap(),
        "Kitchen\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn own_tones_outputs_are_read_with_their_string_ids() {
    let body: Value = serde_json::from_str(
        r#"{"outputs":[{"id":"123456789","name":"Kitchen","type":"AirPlay","selected":true,"volume":33},
                       {"name":"no id"},{"id":7,"name":"Numbered","type":"AirPlay","selected":false,"volume":250}]}"#,
    )
    .unwrap();
    let outputs = parse_outputs(&body);
    assert_eq!(outputs.len(), 2);
    assert_eq!(
        outputs[0],
        Output {
            id: "123456789".to_owned(),
            name: "Kitchen".to_owned(),
            selected: true,
            volume: 33,
            kind: "AirPlay".to_owned()
        }
    );
    assert_eq!((outputs[1].id.as_str(), outputs[1].volume), ("7", 100));
}

#[test]
fn own_tones_password_goes_as_basic_admin() {
    assert_eq!(base64("admin:pw"), "YWRtaW46cHc=");
    assert_eq!(base64("admin:p"), "YWRtaW46cA==");
    assert_eq!(base64("admin:pwd"), "YWRtaW46cHdk");
}

#[test]
fn the_senders_password_is_read_when_needed_so_it_may_come_later() {
    let file = std::env::temp_dir().join(format!(
        "endora-sound-sender-password-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&file);
    let sender = OwnTone::new("http://127.0.0.1:1", Some(file.clone()));
    assert_eq!(
        sender.signed(),
        None,
        "signed with a password that is not there yet"
    );
    std::fs::write(&file, "pw\n").unwrap();
    assert_eq!(sender.signed().as_deref(), Some("Basic YWRtaW46cHc="));
    assert_eq!(OwnTone::new("http://127.0.0.1:1", None).signed(), None);
    let _ = std::fs::remove_file(&file);
}

#[cfg(unix)]
#[test]
fn a_sounding_group_is_written_to_its_pipe_and_a_quiet_one_closes_it() {
    use std::io::Read as _;
    let s = setup();
    let dir = std::env::temp_dir().join(format!("endora-sound-sender-pipe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pipe = dir.join("group.pcm");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let reading = pipe.clone();
    let reader = std::thread::spawn(move || {
        let mut got = Vec::new();
        std::fs::File::open(reading)
            .unwrap()
            .read_to_end(&mut got)
            .unwrap();
        got
    });
    let mut senders = OwnToneSenders::start(
        s.control.hub.clone(),
        &[SenderPlace {
            api: "http://127.0.0.1:1".to_owned(),
            pipe,
        }],
        vec![Box::new(s.fake.clone())],
    );
    senders.play(0, &[1, -2, 3, -4], true);
    std::thread::sleep(Duration::from_millis(200));
    senders.play(0, &[9, 9], false); // quiet: the pipe closes, the reader sees its end
    let got = reader.join().unwrap();
    assert_eq!(got, vec![1, 0, 254, 255, 3, 0, 252, 255]);
}
