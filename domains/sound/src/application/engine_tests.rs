use std::collections::HashMap;

use super::engine::{Change, Engine, Feeds, IDLE_AFTER_FRAMES};
use crate::domain::mix::CHANNELS;
use crate::domain::patchbay::SENDERS;
use crate::domain::{Level, Patchbay, SourceId, SourceKind, SpeakerId, SpeakerKind};

fn level(n: u8) -> Level {
    Level::new(n).unwrap()
}

/// Feeds that hand out a steady value per source, and count what they were asked for.
#[derive(Default)]
struct Steady {
    value: HashMap<SourceId, i16>,
    asked: HashMap<SourceId, usize>,
}

impl Feeds for Steady {
    fn take(&mut self, source: SourceId, samples: usize) -> Vec<i16> {
        *self.asked.entry(source).or_default() += 1;
        match self.value.get(&source) {
            Some(v) => vec![*v; samples],
            None => Vec::new(),
        }
    }
}

struct Home {
    bay: Patchbay,
    phone: SourceId,
    radio: SourceId,
    voice: SourceId,
    kitchen: SpeakerId,
    office: SpeakerId,
    mac: SpeakerId,
    phone_out: SpeakerId,
}

fn home() -> Home {
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
    let voice = bay.add_source("Endora", SourceKind::Voice, None).unwrap();
    let kitchen = bay
        .add_speaker("Kitchen", SpeakerKind::AirPlay, None)
        .unwrap();
    let office = bay
        .add_speaker("Office", SpeakerKind::AirPlay, None)
        .unwrap();
    let mac = bay
        .add_speaker("Mac", SpeakerKind::AirPlay, Some("mac"))
        .unwrap();
    let phone_out = bay
        .add_speaker("iPhone", SpeakerKind::Stream, Some("iphone"))
        .unwrap();
    Home {
        bay,
        phone,
        radio,
        voice,
        kitchen,
        office,
        mac,
        phone_out,
    }
}

fn senders_of(changes: &[Change]) -> Vec<(usize, Vec<SpeakerId>)> {
    changes
        .iter()
        .filter_map(|c| match c {
            Change::Sender { sender, speakers } => Some((*sender, speakers.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_new_group_gets_a_sender_for_its_airplay_speakers_and_a_stream_only_one_gets_none() {
    let mut h = home();
    h.bay.wire(h.radio, h.kitchen, level(60)).unwrap();
    h.bay.wire(h.radio, h.phone_out, level(10)).unwrap();
    let mut engine = Engine::default();
    let changes = engine.plan(&h.bay);
    assert_eq!(senders_of(&changes), vec![(0, vec![h.kitchen])]);
    let runs = engine.running();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs.iter().filter(|r| r.sender.is_some()).count(), 1);
}

#[test]
fn moving_a_level_keeps_the_group_its_sender_and_changes_nothing_on_the_speakers() {
    let mut h = home();
    h.bay.wire(h.radio, h.kitchen, level(60)).unwrap();
    h.bay.wire(h.radio, h.office, level(60)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    let before = engine.running()[0].run;
    h.bay.set_all(h.radio, level(20)).unwrap();
    let changes = engine.plan(&h.bay);
    assert!(
        changes.is_empty(),
        "a slider moved and the speakers were touched: {changes:?}"
    );
    assert_eq!(engine.running()[0].run, before);
    assert_eq!(engine.running()[0].mix[0].level, level(20));
}

#[test]
fn a_speaker_moving_to_another_group_is_let_go_by_its_old_sender_first() {
    let mut h = home();
    // Classical in the kitchen and the office; the phone in the Mac.
    h.bay.wire(h.radio, h.kitchen, level(60)).unwrap();
    h.bay.wire(h.radio, h.office, level(60)).unwrap();
    h.bay.wire(h.phone, h.mac, level(90)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    // The office leaves classical and joins the Mac's mix.
    h.bay.unwire(h.radio, h.office);
    h.bay.wire(h.phone, h.office, level(90)).unwrap();
    let changes = senders_of(&engine.plan(&h.bay));
    let let_go = changes
        .iter()
        .position(|(s, sp)| *s == 0 && !sp.contains(&h.office));
    let taken = changes
        .iter()
        .position(|(s, sp)| *s == 1 && sp.contains(&h.office));
    assert!(let_go.is_some() && taken.is_some(), "{changes:?}");
    assert!(let_go < taken, "taken before it was let go: {changes:?}");
}

#[test]
fn a_group_that_goes_frees_its_sender_for_the_next() {
    let mut h = home();
    h.bay.wire(h.radio, h.kitchen, level(60)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    h.bay.unwire(h.radio, h.kitchen);
    h.bay.wire(h.phone, h.office, level(90)).unwrap();
    let changes = engine.plan(&h.bay);
    // The freed sender is the one the new group takes, after it let the kitchen go.
    let released = changes.iter().position(|c| {
        *c == Change::Released { sender: 0 }
            || matches!(c, Change::Sender { sender: 0, speakers } if speakers.is_empty())
    });
    assert!(
        released.is_some(),
        "the kitchen was never let go: {changes:?}"
    );
    let taken = changes.iter().position(
        |c| matches!(c, Change::Sender { sender: 0, speakers } if speakers == &vec![h.office]),
    );
    assert!(released < taken, "{changes:?}");
}

#[test]
fn never_more_senders_than_there_are() {
    let mut bay = Patchbay::default();
    let radio = bay
        .add_source(
            "Radio",
            SourceKind::Radio("https://radio.example.com/a".to_owned()),
            None,
        )
        .unwrap();
    let speakers: Vec<SpeakerId> = (0..SENDERS)
        .map(|i| {
            bay.add_speaker(&format!("HomePod {i}"), SpeakerKind::AirPlay, None)
                .unwrap()
        })
        .collect();
    let mut engine = Engine::default();
    for round in 0..3u8 {
        for (i, s) in speakers.iter().enumerate() {
            bay.wire(radio, *s, level(10 + round * 10 + u8::try_from(i).unwrap()))
                .unwrap();
        }
        engine.plan(&bay);
        let mut used: Vec<usize> = engine.running().iter().filter_map(|r| r.sender).collect();
        used.sort_unstable();
        assert_eq!(used, (0..SENDERS).collect::<Vec<_>>());
    }
}

#[test]
fn a_tick_mixes_each_group_at_its_own_levels_and_takes_each_source_once() {
    let mut h = home();
    // Classical everywhere at 10, the phone over it at 90 in the office.
    h.bay.wire(h.radio, h.kitchen, level(10)).unwrap();
    h.bay.wire(h.radio, h.office, level(10)).unwrap();
    h.bay.wire(h.phone, h.office, level(90)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    let mut feeds = Steady::default();
    feeds.value.insert(h.radio, 10_000);
    feeds.value.insert(h.phone, 10_000);
    let played = engine.tick(441, &mut feeds);
    assert_eq!(played.len(), 2);
    let kitchen = played
        .iter()
        .find(|p| p.speakers.contains(&h.kitchen))
        .unwrap();
    let office = played
        .iter()
        .find(|p| p.speakers.contains(&h.office))
        .unwrap();
    assert_eq!(kitchen.samples.len(), 441 * CHANNELS);
    assert_eq!(kitchen.samples[0], 1_000);
    assert_eq!(office.samples[0], 10_000);
    assert_eq!(
        feeds.asked[&h.radio], 1,
        "a source feeding two groups was read twice"
    );
}

#[test]
fn a_group_with_nothing_to_play_goes_quiet_and_wakes_when_sound_comes() {
    let mut h = home();
    h.bay.wire(h.phone, h.kitchen, level(90)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    let mut feeds = Steady::default();
    // Nothing from the phone: not sounding from the start.
    assert!(!engine.tick(441, &mut feeds)[0].sounding);
    feeds.value.insert(h.phone, 5_000);
    assert!(engine.tick(441, &mut feeds)[0].sounding);
    // Then the phone stops: still sounding for a while, then quiet.
    feeds.value.clear();
    assert!(engine.tick(441, &mut feeds)[0].sounding);
    let mut quiet = false;
    for _ in 0..(IDLE_AFTER_FRAMES / 441 + 2) {
        quiet = !engine.tick(441, &mut feeds)[0].sounding;
    }
    assert!(quiet, "it never went quiet");
}

#[test]
fn silence_from_a_source_is_not_sound() {
    // A receiver may write silence while the phone is paused; that must not hold speakers.
    let mut h = home();
    h.bay.wire(h.phone, h.kitchen, level(90)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    let mut feeds = Steady::default();
    feeds.value.insert(h.phone, 0);
    assert!(!engine.tick(441, &mut feeds)[0].sounding);
}

#[test]
fn endoras_voice_ducks_the_music_in_its_groups() {
    let mut h = home();
    h.bay.wire(h.radio, h.kitchen, level(100)).unwrap();
    h.bay.wire(h.voice, h.kitchen, level(100)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    let mut feeds = Steady::default();
    feeds.value.insert(h.radio, 10_000);
    feeds.value.insert(h.voice, 2_000);
    let mut last = 0;
    for _ in 0..50 {
        last = *engine.tick(441, &mut feeds)[0].samples.last().unwrap();
    }
    // Ducked to a quarter, plus the voice.
    assert!((i32::from(last) - 4_500).abs() < 50, "{last}");
}

#[test]
fn a_borrowed_speaker_is_let_go_and_taken_back_when_free() {
    let mut h = home();
    h.bay.wire(h.radio, h.kitchen, level(60)).unwrap();
    h.bay.wire(h.radio, h.office, level(60)).unwrap();
    let mut engine = Engine::default();
    engine.plan(&h.bay);
    h.bay.set_borrowed(h.office, true).unwrap();
    assert_eq!(senders_of(&engine.plan(&h.bay)), vec![(0, vec![h.kitchen])]);
    h.bay.set_borrowed(h.office, false).unwrap();
    assert_eq!(
        senders_of(&engine.plan(&h.bay)),
        vec![(0, vec![h.kitchen, h.office])]
    );
}
