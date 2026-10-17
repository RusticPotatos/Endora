use super::*;

/// A patchbay with the owner's usual things: the phone and a radio station as sources, the
/// phone's stream, the kitchen HomePod and the office HomePod as speakers.
fn usual() -> (
    Patchbay,
    SourceId,
    SourceId,
    SpeakerId,
    SpeakerId,
    SpeakerId,
) {
    let mut bay = Patchbay::default();
    let phone = bay
        .add_source("Endora · iPhone", SourceKind::AirPlay, Some("iphone"))
        .unwrap();
    let radio = bay
        .add_source(
            "Classical",
            SourceKind::Radio("https://radio.example.com/classical".to_owned()),
            None,
        )
        .unwrap();
    let phone_out = bay
        .add_speaker("iPhone", SpeakerKind::Stream, Some("iphone"))
        .unwrap();
    let kitchen = bay
        .add_speaker("Kitchen", SpeakerKind::AirPlay, None)
        .unwrap();
    let office = bay
        .add_speaker("Office", SpeakerKind::AirPlay, None)
        .unwrap();
    (bay, phone, radio, phone_out, kitchen, office)
}

fn level(n: u8) -> Level {
    Level::new(n).unwrap()
}

#[test]
fn a_level_is_zero_to_a_hundred() {
    assert_eq!(Level::new(0).map(Level::get), Some(0));
    assert_eq!(Level::new(100).map(Level::get), Some(100));
    assert_eq!(Level::new(101), None);
}

#[test]
fn a_device_is_never_wired_to_itself() {
    let (mut bay, phone, radio, phone_out, kitchen, _) = usual();
    assert_eq!(
        bay.wire(phone, phone_out, level(90)),
        Err(Refusal::ItsOwnSpeaker)
    );
    // The same device's source goes anywhere else, and anything else goes to it.
    assert_eq!(bay.wire(phone, kitchen, level(90)), Ok(()));
    assert_eq!(bay.wire(radio, phone_out, level(10)), Ok(()));
}

#[test]
fn wiring_something_unknown_is_refused() {
    let (mut bay, phone, _, _, kitchen, _) = usual();
    assert_eq!(
        bay.wire(SourceId(99), kitchen, level(5)),
        Err(Refusal::NoSuchSource)
    );
    assert_eq!(
        bay.wire(phone, SpeakerId(99), level(5)),
        Err(Refusal::NoSuchSpeaker)
    );
}

#[test]
fn a_wire_is_set_again_not_doubled_and_comes_off() {
    let (mut bay, _, radio, _, kitchen, _) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.wire(radio, kitchen, level(30)).unwrap();
    assert_eq!(
        bay.wires(),
        vec![Wire {
            source: radio,
            speaker: kitchen,
            level: level(30)
        }]
    );
    assert!(bay.unwire(radio, kitchen));
    assert!(!bay.unwire(radio, kitchen));
    assert!(bay.wires().is_empty());
}

#[test]
fn a_source_sets_all_its_wires_and_each_stays_its_own() {
    let (mut bay, phone, radio, phone_out, kitchen, office) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.wire(radio, office, level(40)).unwrap();
    bay.wire(phone, office, level(90)).unwrap();
    assert_eq!(bay.set_all(radio, level(10)), Ok(2));
    let radio_levels: Vec<u8> = bay
        .wires()
        .iter()
        .filter(|w| w.source == radio)
        .map(|w| w.level.get())
        .collect();
    assert_eq!(radio_levels, vec![10, 10]);
    // The phone's wire is untouched, and setting all adds no wire that was not there.
    assert!(
        bay.wires()
            .iter()
            .any(|w| w.source == phone && w.level.get() == 90)
    );
    assert!(!bay.wires().iter().any(|w| w.speaker == phone_out));
    // One of them can still go its own way.
    bay.wire(radio, kitchen, level(60)).unwrap();
    assert!(bay.wires().contains(&Wire {
        source: radio,
        speaker: kitchen,
        level: level(60)
    }));
}

#[test]
fn speakers_with_the_same_wires_play_as_one_group() {
    // Evening: classical everywhere at 10, and the phone over it at 90 in the office only.
    let (mut bay, phone, radio, _, kitchen, office) = usual();
    let mac = bay
        .add_speaker("Mac", SpeakerKind::AirPlay, Some("mac"))
        .unwrap();
    for s in [kitchen, office, mac] {
        bay.wire(radio, s, level(10)).unwrap();
    }
    bay.wire(phone, office, level(90)).unwrap();
    bay.wire(phone, mac, level(90)).unwrap();
    let groups = bay.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].speakers, vec![kitchen]);
    assert_eq!(groups[0].mix, vec![(radio, level(10))]);
    assert_eq!(groups[1].speakers, vec![office, mac]);
    assert_eq!(groups[1].mix, vec![(phone, level(90)), (radio, level(10))]);
}

#[test]
fn a_different_level_is_a_different_mix() {
    let (mut bay, _, radio, _, kitchen, office) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.wire(radio, office, level(10)).unwrap();
    assert_eq!(bay.groups().len(), 2);
}

#[test]
fn a_speaker_with_no_wires_plays_nothing() {
    let (bay, ..) = usual();
    assert!(bay.groups().is_empty());
}

#[test]
fn a_borrowed_speaker_keeps_its_wires_but_leaves_its_group_until_it_is_free() {
    let (mut bay, phone, _, _, kitchen, office) = usual();
    bay.wire(phone, kitchen, level(80)).unwrap();
    bay.wire(phone, office, level(80)).unwrap();
    assert_eq!(bay.set_borrowed(office, true), Ok(()));
    assert_eq!(bay.groups().len(), 1);
    assert_eq!(bay.groups()[0].speakers, vec![kitchen]);
    assert_eq!(
        bay.wires().len(),
        2,
        "its wires are kept for when it is free"
    );
    bay.set_borrowed(office, false).unwrap();
    assert_eq!(bay.groups()[0].speakers, vec![kitchen, office]);
}

#[test]
fn no_more_airplay_groups_than_there_are_senders() {
    let mut bay = Patchbay::default();
    let radio = bay
        .add_source(
            "Radio",
            SourceKind::Radio("https://radio.example.com/a".to_owned()),
            None,
        )
        .unwrap();
    let speakers: Vec<SpeakerId> = (0..=SENDERS)
        .map(|i| {
            bay.add_speaker(&format!("HomePod {i}"), SpeakerKind::AirPlay, None)
                .unwrap()
        })
        .collect();
    // Each at its own level is its own group: the senders run out after SENDERS of them.
    for (i, s) in speakers.iter().take(SENDERS).enumerate() {
        bay.wire(radio, *s, level(10 + u8::try_from(i).unwrap()))
            .unwrap();
    }
    assert_eq!(
        bay.wire(radio, speakers[SENDERS], level(90)),
        Err(Refusal::NoFreeSender)
    );
    // Joining a group that already plays needs no new sender.
    assert_eq!(bay.wire(radio, speakers[SENDERS], level(10)), Ok(()));
    // And stream speakers need no sender at all.
    let phone = bay
        .add_speaker("iPhone", SpeakerKind::Stream, Some("iphone"))
        .unwrap();
    assert_eq!(bay.wire(radio, phone, level(77)), Ok(()));
}

#[test]
fn freeing_a_borrowed_speaker_never_takes_a_sender_that_is_not_there() {
    let mut bay = Patchbay::default();
    let radio = bay
        .add_source(
            "Radio",
            SourceKind::Radio("https://radio.example.com/a".to_owned()),
            None,
        )
        .unwrap();
    let speakers: Vec<SpeakerId> = (0..=SENDERS)
        .map(|i| {
            bay.add_speaker(&format!("HomePod {i}"), SpeakerKind::AirPlay, None)
                .unwrap()
        })
        .collect();
    bay.wire(radio, speakers[SENDERS], level(99)).unwrap();
    bay.set_borrowed(speakers[SENDERS], true).unwrap();
    for (i, s) in speakers.iter().take(SENDERS).enumerate() {
        bay.wire(radio, *s, level(10 + u8::try_from(i).unwrap()))
            .unwrap();
    }
    // Free again, it would make one group too many: it stays out until a sender frees up,
    // and says so.
    assert_eq!(
        bay.set_borrowed(speakers[SENDERS], false),
        Err(Refusal::NoFreeSender)
    );
    assert!(bay.speaker(speakers[SENDERS]).unwrap().borrowed);
}

#[test]
fn a_radio_station_is_a_web_address_and_nothing_else() {
    let mut bay = Patchbay::default();
    for bad in [
        "file:///etc/passwd",
        "concat:/a|/b",
        "ftp://x.example.com/a",
        "https://",
        "https://a b.example.com",
        "",
    ] {
        assert_eq!(
            bay.add_source("Bad", SourceKind::Radio(bad.to_owned()), None),
            Err(Refusal::NotAWebAddress),
            "{bad}"
        );
    }
    assert!(
        bay.add_source(
            "Good",
            SourceKind::Radio("http://radio.example.com:8000/live".to_owned()),
            None
        )
        .is_ok()
    );
}

#[test]
fn names_are_short_and_not_blank_and_not_taken() {
    let mut bay = Patchbay::default();
    assert_eq!(
        bay.add_speaker("  ", SpeakerKind::AirPlay, None),
        Err(Refusal::BadName)
    );
    assert_eq!(
        bay.add_speaker(&"x".repeat(41), SpeakerKind::AirPlay, None),
        Err(Refusal::BadName)
    );
    bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None)
        .unwrap();
    assert_eq!(
        bay.add_speaker(" kitchen ", SpeakerKind::AirPlay, None),
        Err(Refusal::NameTaken)
    );
}

#[test]
fn removing_a_source_or_a_speaker_takes_its_wires_with_it() {
    let (mut bay, phone, radio, _, kitchen, office) = usual();
    bay.wire(phone, kitchen, level(50)).unwrap();
    bay.wire(radio, office, level(50)).unwrap();
    assert!(bay.remove_source(phone));
    assert_eq!(bay.wires().len(), 1);
    assert!(bay.remove_speaker(office));
    assert!(bay.wires().is_empty());
    assert!(!bay.remove_speaker(office));
}

#[test]
fn a_setup_saves_the_wires_and_applying_it_puts_them_back() {
    let (mut bay, phone, radio, _, kitchen, office) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.save_setup("Kitchen mornings").unwrap();
    bay.wire(phone, office, level(90)).unwrap();
    bay.unwire(radio, kitchen);
    assert_eq!(bay.apply_setup("kitchen mornings"), Ok(()));
    assert_eq!(
        bay.wires(),
        vec![Wire {
            source: radio,
            speaker: kitchen,
            level: level(60)
        }]
    );
    assert_eq!(bay.apply_setup("Night"), Err(Refusal::NoSuchSetup));
}

#[test]
fn a_setup_skips_what_has_gone_since_it_was_saved() {
    let (mut bay, phone, radio, _, kitchen, office) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.wire(phone, office, level(90)).unwrap();
    bay.save_setup("Both").unwrap();
    bay.remove_speaker(office);
    bay.apply_setup("Both").unwrap();
    assert_eq!(
        bay.wires(),
        vec![Wire {
            source: radio,
            speaker: kitchen,
            level: level(60)
        }]
    );
}

#[test]
fn a_setup_that_needs_more_senders_than_there_are_is_refused_whole() {
    let mut bay = Patchbay::default();
    let radio = bay
        .add_source(
            "Radio",
            SourceKind::Radio("https://radio.example.com/a".to_owned()),
            None,
        )
        .unwrap();
    let speakers: Vec<SpeakerId> = (0..=SENDERS)
        .map(|i| {
            bay.add_speaker(&format!("HomePod {i}"), SpeakerKind::AirPlay, None)
                .unwrap()
        })
        .collect();
    for (i, s) in speakers.iter().take(SENDERS).enumerate() {
        bay.wire(radio, *s, level(10 + u8::try_from(i).unwrap()))
            .unwrap();
    }
    bay.save_setup("Four").unwrap();
    // Built by hand into one that would need SENDERS + 1 senders, as a stored setup could be.
    let mut too_many = bay.setup("Four").unwrap().to_vec();
    too_many.push(Wire {
        source: radio,
        speaker: speakers[SENDERS],
        level: level(99),
    });
    bay.restore_setup("Too many", too_many);
    let before = bay.wires();
    assert_eq!(bay.apply_setup("Too many"), Err(Refusal::NoFreeSender));
    assert_eq!(bay.wires(), before, "nothing changed");
}

#[test]
fn a_speakers_volume_is_its_own() {
    let (mut bay, _, radio, _, kitchen, _) = usual();
    bay.wire(radio, kitchen, level(60)).unwrap();
    bay.set_volume(kitchen, level(40)).unwrap();
    assert_eq!(bay.speaker(kitchen).unwrap().volume, level(40));
    // Volume is how loud the speaker plays its mix, not part of the mix.
    assert_eq!(bay.groups()[0].mix, vec![(radio, level(60))]);
    assert_eq!(
        bay.set_volume(SpeakerId(99), level(1)),
        Err(Refusal::NoSuchSpeaker)
    );
}

#[test]
fn every_change_moves_the_revision_on_and_a_refusal_does_not() {
    let (mut bay, phone, _, phone_out, kitchen, _) = usual();
    let r = bay.revision();
    bay.wire(phone, kitchen, level(1)).unwrap();
    assert!(bay.revision() > r);
    let r = bay.revision();
    let _ = bay.wire(phone, phone_out, level(1));
    assert_eq!(bay.revision(), r);
}
