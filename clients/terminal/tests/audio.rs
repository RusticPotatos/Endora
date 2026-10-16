use endora_terminal::audio::{RATE, STEPS, tone};

fn samples(bytes: &[u8]) -> Vec<(i16, i16)> {
    bytes
        .chunks_exact(4)
        .map(|f| {
            (
                i16::from_le_bytes([f[0], f[1]]),
                i16::from_le_bytes([f[2], f[3]]),
            )
        })
        .collect()
}

#[test]
fn a_tone_is_silent_in_the_ear_that_is_not_being_tested() {
    let left = samples(&tone(true, false, 500));
    let right = samples(&tone(false, true, 500));
    let both = samples(&tone(true, true, 500));
    assert_eq!(left.len(), (RATE / 2) as usize, "half a second of frames");
    assert!(left.iter().all(|s| s.1 == 0) && left.iter().any(|s| s.0 != 0));
    assert!(right.iter().all(|s| s.0 == 0) && right.iter().any(|s| s.1 != 0));
    assert!(both.iter().any(|s| s.0 != 0) && both.iter().any(|s| s.1 != 0));
}

#[test]
fn the_tone_is_gentle_and_starts_and_ends_softly() {
    let t = samples(&tone(true, true, 400));
    let peak = t.iter().map(|s| s.0.unsigned_abs()).max().unwrap();
    assert!((3000..=12000).contains(&peak), "comfortable volume: {peak}");
    assert!(t[0].0.unsigned_abs() < 500, "no click at the start");
    assert!(t[t.len() - 1].0.unsigned_abs() < 500, "no click at the end");
}

#[test]
fn the_sound_test_goes_left_then_right_then_both() {
    let names: Vec<&str> = STEPS.iter().map(|s| s.0).collect();
    assert_eq!(names, ["left ear", "right ear", "both ears"]);
    assert_eq!((STEPS[0].1, STEPS[0].2), (true, false));
    assert_eq!((STEPS[1].1, STEPS[1].2), (false, true));
    assert_eq!((STEPS[2].1, STEPS[2].2), (true, true));
}

fn health(card: bool, device: bool, sink: bool) -> endora_terminal::audio::Health {
    endora_terminal::audio::Health { card, device, sink }
}

/// The checks that only look again: a dropout is given a moment to fix itself.
fn grace(guard: &mut endora_terminal::audio::AudioGuard, h: endora_terminal::audio::Health) {
    use endora_terminal::audio::{GRACE, Step};
    for _ in 0..GRACE {
        assert_eq!(
            guard.observe(h),
            Some(Step::Recheck),
            "give it a moment first"
        );
    }
}

#[test]
fn a_brief_dropout_that_fixes_itself_is_never_touched() {
    use endora_terminal::audio::{AudioGuard, Step};
    let mut guard = AudioGuard::default();
    assert_eq!(guard.observe(health(true, true, true)), None);
    assert_eq!(
        guard.observe(health(false, true, false)),
        Some(Step::Recheck)
    );
    assert_eq!(
        guard.observe(health(true, true, false)),
        Some(Step::Recheck)
    );
    assert_eq!(
        guard.observe(health(true, true, true)),
        None,
        "it came back by itself"
    );
    // The next dropout gets its own full grace period.
    assert_eq!(
        guard.observe(health(false, true, false)),
        Some(Step::Recheck)
    );
}

#[test]
fn a_working_dongle_that_comes_back_half_started_is_reset_a_few_times_then_reported() {
    use endora_terminal::audio::{AudioGuard, Step};
    let (healthy, half_started) = (health(true, true, true), health(false, true, false));
    let mut guard = AudioGuard::default();
    assert_eq!(
        guard.observe(half_started),
        None,
        "never seen working: leave it alone"
    );
    assert_eq!(guard.observe(healthy), None);
    grace(&mut guard, half_started);
    for _ in 0..4 {
        assert_eq!(guard.observe(half_started), Some(Step::ResetDevice));
    }
    assert_eq!(
        guard.observe(half_started),
        Some(Step::GiveUp),
        "then asks for a replug"
    );
    assert_eq!(
        guard.observe(half_started),
        None,
        "and does not keep nagging"
    );
    // Plugging it in by hand fixes it, and the next problem gets a fresh set of tries.
    assert_eq!(guard.observe(healthy), None);
    grace(&mut guard, half_started);
    assert_eq!(guard.observe(half_started), Some(Step::ResetDevice));
}

#[test]
fn a_sound_card_without_an_output_restarts_the_audio_session_and_an_unplugged_dongle_is_left_alone()
{
    use endora_terminal::audio::{AudioGuard, Step};
    let (healthy, no_output, gone) = (
        health(true, true, true),
        health(true, true, false),
        health(false, false, false),
    );
    let mut guard = AudioGuard::default();
    assert_eq!(guard.observe(healthy), None);
    grace(&mut guard, no_output);
    assert_eq!(guard.observe(no_output), Some(Step::RestartSession));
    assert_eq!(guard.observe(healthy), None);
    assert_eq!(
        guard.observe(gone),
        None,
        "unplugged on purpose (or still re-appearing)"
    );
    assert_eq!(guard.observe(gone), None);
    // The card is there but the output is not: restart the session twice, then reset the
    // dongle itself twice, then ask for a replug.
    grace(&mut guard, no_output);
    assert_eq!(guard.observe(no_output), Some(Step::RestartSession));
    assert_eq!(guard.observe(no_output), Some(Step::RestartSession));
    assert_eq!(guard.observe(no_output), Some(Step::ResetDevice));
    assert_eq!(guard.observe(no_output), Some(Step::ResetDevice));
    assert_eq!(
        guard.observe(no_output),
        Some(Step::GiveUp),
        "nothing helped"
    );
    assert_eq!(guard.observe(no_output), None);
}

#[test]
fn a_dongle_that_refuses_the_reset_is_reported_at_once_and_not_retried() {
    use endora_terminal::audio::{AudioGuard, Step};
    let (healthy, half_started) = (health(true, true, true), health(false, true, false));
    let mut guard = AudioGuard::default();
    assert_eq!(guard.observe(healthy), None);
    grace(&mut guard, half_started);
    assert_eq!(guard.observe(half_started), Some(Step::ResetDevice));
    guard.stuck();
    assert_eq!(
        guard.observe(half_started),
        None,
        "no pointless retries once it is stuck"
    );
    assert_eq!(guard.observe(healthy), None);
    grace(&mut guard, half_started);
    assert_eq!(
        guard.observe(half_started),
        Some(Step::ResetDevice),
        "plugging it in starts fresh"
    );
}

#[test]
fn only_hardware_arrivals_and_departures_count_as_audio_events() {
    use endora_terminal::audio::is_hardware_event;
    assert!(is_hardware_event(
        "UDEV  [1234.5] add      /devices/platform/soc/3f980000.usb/usb1/1-1/1-1.1 (usb)"
    ));
    assert!(is_hardware_event(
        "UDEV  [1234.9] remove   /devices/platform/soc/3f980000.usb/usb1/1-1/1-1.1/sound/card1 (sound)"
    ));
    assert!(is_hardware_event(
        "UDEV  [9.1] bind     /devices/x/1-1.1 (usb)"
    ));
    assert!(is_hardware_event(
        "KERNEL[9.1] unbind   /devices/x/1-1.1 (usb)"
    ));
    assert!(!is_hardware_event(
        "UDEV  [9.1] change   /devices/x/card1 (sound)"
    ));
    assert!(!is_hardware_event(
        "UDEV  [9.1] add      /devices/x/input5 (input)"
    ));
    assert!(!is_hardware_event(
        "monitor will print the received events for:"
    ));
    assert!(!is_hardware_event(""));
}

#[test]
fn only_outputs_appearing_or_disappearing_count_as_audio_server_events() {
    use endora_terminal::audio::is_sink_event;
    assert!(is_sink_event("Event 'new' on sink #77"));
    assert!(is_sink_event("Event 'remove' on sink #77"));
    assert!(is_sink_event("Event 'remove' on card #5"));
    assert!(
        !is_sink_event("Event 'change' on sink #77"),
        "volume or state changes are noise"
    );
    assert!(
        !is_sink_event("Event 'new' on sink-input #3"),
        "a stream starting is not an output"
    );
    assert!(!is_sink_event("Event 'remove' on client #3"));
    assert!(!is_sink_event(""));
}

#[test]
fn a_burst_of_events_causes_one_check_after_things_settle() {
    use endora_terminal::audio::settle_once;
    use std::time::{Duration, Instant};
    let (tx, rx) = std::sync::mpsc::channel();
    for _ in 0..6 {
        tx.send(()).unwrap();
    }
    let started = Instant::now();
    assert!(
        settle_once(&rx, Duration::from_millis(150)),
        "one check for the burst"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "waits for it to settle"
    );
    assert!(rx.try_recv().is_err(), "the burst was drained");
    drop(tx);
    assert!(
        !settle_once(&rx, Duration::from_millis(10)),
        "stops when the watcher is gone"
    );
}
