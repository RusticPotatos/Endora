use endora_terminal::{
    presentation::{actions, activity},
    wheel::{scroll_by, wheel_delta},
};
use serde_json::json;

#[test]
fn typing_area_contains_only_the_draft_and_keeps_cursor_visible() {
    use endora_terminal::composer::{draw, window};
    use ratatui::{Terminal, backend::TestBackend};
    let mut terminal = Terminal::new(TestBackend::new(24, 3)).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), "hello", 5);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row: String = (0..24).map(|x| buffer[(x, 1)].symbol()).collect();
    assert_eq!(row.trim_matches(['│', ' ']), "hello");
    let (visible, cursor) = window("abcdefghijk", 11, 6);
    assert_eq!(visible, "ghijk");
    assert_eq!(cursor, 5);
    let (visible, cursor) = window("ab界cd", "ab界".len(), 3);
    assert_eq!(visible, "界cd");
    assert_eq!(cursor, 2);
}

#[test]
fn linux_console_does_not_get_terminal_mouse_control_codes() {
    assert!(!endora_terminal::wheel::terminal_mouse_supported(
        "linux", false
    ));
    assert!(!endora_terminal::wheel::terminal_mouse_supported(
        "xterm", true
    ));
    assert!(endora_terminal::wheel::terminal_mouse_supported(
        "xterm-256color",
        false
    ));
}

#[test]
fn metadata_is_compact_text_without_payloads() {
    let value = json!({"actions_taken":[{"skill":"lights", "confirmed":true,"outcome":"done","observed":{"huge":"hidden"}}],"steps":[{"payload":"hidden"}],"sources":[]});
    assert_eq!(actions(&value), "\nActions:\n  lights: confirmed\n");
    assert_eq!(
        activity(&json!([{"kind":"action","summary":"Lights checked","payload":"hidden"}])),
        "action: Lights checked\n"
    );
    assert!(!actions(&json!({"steps":[{"payload":"hidden"}]})).contains("hidden"));
}

#[test]
fn wheel_direction_and_bounds() {
    let mut record = [0u8; 24];
    record[16..18].copy_from_slice(&2u16.to_ne_bytes());
    record[18..20].copy_from_slice(&8u16.to_ne_bytes());
    record[20..24].copy_from_slice(&1i32.to_ne_bytes());
    assert_eq!(wheel_delta(&record), 1);
    assert_eq!(scroll_by(10, 1, 20), 7);
    assert_eq!(scroll_by(19, -1, 20), 20);
    assert_eq!(scroll_by(0, i32::MAX, 20), 0);
    record[18..20].copy_from_slice(&11u16.to_ne_bytes());
    assert_eq!(wheel_delta(&record), 0); // high-resolution duplicate is ignored
    assert_eq!(wheel_delta(&record[..12]), 0);
}

#[test]
fn q20_vertical_pointer_motion_scrolls_without_horizontal_jitter() {
    use endora_terminal::wheel::Q20Scroll;
    fn event(code: u16, value: i32) -> [u8; 24] {
        let mut record = [0; 24];
        record[16..18].copy_from_slice(&2u16.to_ne_bytes());
        record[18..20].copy_from_slice(&code.to_ne_bytes());
        record[20..24].copy_from_slice(&value.to_ne_bytes());
        record
    }
    let mut control = Q20Scroll::default();
    assert_eq!(control.event(&event(0, 100)), 0);
    assert_eq!(control.event(&event(1, -6)), 0);
    assert_eq!(control.event(&event(1, -6)), 1);
    assert_eq!(control.event(&event(1, 24)), -2);
    assert_eq!(control.event(&event(8, 1)), 1);
    assert_eq!(control.event(&event(11, 120)), 0);
    assert_eq!(control.event(&[0; 12]), 0);
    assert!(control.event(&event(1, i32::MIN)) > 0);
}

#[test]
fn q20_sideways_motion_becomes_left_right_steps_for_the_home_grid() {
    use endora_terminal::wheel::Q20Scroll;
    fn event(code: u16, value: i32) -> [u8; 24] {
        let mut record = [0; 24];
        record[16..18].copy_from_slice(&2u16.to_ne_bytes());
        record[18..20].copy_from_slice(&code.to_ne_bytes());
        record[20..24].copy_from_slice(&value.to_ne_bytes());
        record
    }
    let mut control = Q20Scroll::default();
    assert_eq!(control.horizontal(&event(0, 10)), 0);
    assert_eq!(control.horizontal(&event(0, 10)), 1);
    assert_eq!(control.horizontal(&event(0, -40)), -2);
    // Vertical motion, wheel events and short records never become sideways steps.
    assert_eq!(control.horizontal(&event(1, 500)), 0);
    assert_eq!(control.horizontal(&event(8, 1)), 0);
    assert_eq!(control.horizontal(&[0; 12]), 0);
    let _ = control.horizontal(&event(0, i32::MIN));
}

#[test]
fn pointer_jitter_and_click_can_wake_without_becoming_a_scroll_action() {
    use endora_terminal::wheel::activity_event;
    let mut event = [0u8; 24];
    event[16..18].copy_from_slice(&2u16.to_ne_bytes());
    event[18..20].copy_from_slice(&0u16.to_ne_bytes());
    event[20..24].copy_from_slice(&1i32.to_ne_bytes());
    assert!(activity_event(&event));
    assert_eq!(wheel_delta(&event), 0);
    event[16..18].copy_from_slice(&1u16.to_ne_bytes());
    event[18..20].copy_from_slice(&272u16.to_ne_bytes());
    assert!(activity_event(&event));
    assert_eq!(wheel_delta(&event), 0);
    event[20..24].copy_from_slice(&0i32.to_ne_bytes());
    assert!(!activity_event(&event));
}

fn dash() -> serde_json::Value {
    json!({
        "stats": ["CPU 10%", "Mem 38%"],
        "alerts": ["DiskLow"],
        "groups": [
            {"name":"Media","services":[
                {"name":"Overseerr","state":"running","id":"abc"},
                {"name":"Plex","state":"unhealthy"},
                {"name":"Router","state":"unknown"},
                {"name":"Old","state":"stopped"}]},
            {"name":"Empty","services":[]},
            {"name":"AI","services":[{"name":"Ollama","state":"running"}]}
        ]
    })
}

#[test]
fn home_screen_is_a_grid_per_group_with_a_key_for_each_service() {
    use endora_terminal::presentation::{dash_key_index, dash_services, home};
    let value = dash();
    assert_eq!(dash_services(&value).len(), 5);
    let (text, line) = home(&value, "12:30", 1, 40);
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows[0], "12:30 | CPU 10% | Mem 38%");
    // Alerts live in their own group (built by the node), not as lines above it.
    assert!(!text.contains("ALERT"));
    assert!(rows.contains(&"Media") && rows.contains(&"AI") && !rows.contains(&"Empty"));
    // Two services per row at this width, left to right, each labelled with its key.
    let first = rows.iter().find(|r| r.contains("Overseerr")).unwrap();
    assert!(
        first.contains(" a + Overseerr") && first.contains(">b ! Plex"),
        "{text}"
    );
    let second = rows.iter().find(|r| r.contains("Router")).unwrap();
    assert!(
        second.contains(" c ? Router") && second.contains(" d - Old"),
        "{text}"
    );
    assert!(rows.iter().all(|r| r.chars().count() <= 40), "{text}");
    assert_eq!(rows[line], *first);
    // Keys skip the letters the console already uses (k lock, p pocket, u unlock, v reconcile).
    assert_eq!(dash_key_index('a'), Some(0));
    assert_eq!(dash_key_index('j'), Some(9));
    assert_eq!(dash_key_index('l'), Some(10));
    for used in ['k', 'p', 'u', 'v'] {
        assert_eq!(dash_key_index(used), None);
    }
    assert_eq!(dash_key_index('9'), Some(31));
    assert!(
        home(&json!({}), "12:30", 0, 40)
            .0
            .contains("No dashboard data")
    );
    // With the clock and battery moved to the top bar, the first line is just the figures.
    let (text, _) = home(&value, "", 1, 40);
    assert_eq!(text.lines().next().unwrap(), "CPU 10% | Mem 38%");
}

#[test]
fn status_bar_right_side_is_clock_then_battery() {
    use endora_terminal::presentation::status_right;
    assert_eq!(
        status_right("VPN  Wi-Fi", "15:24", "Bat 78% ~5h10m"),
        "VPN  Wi-Fi  15:24  Bat 78% ~5h10m"
    );
    assert_eq!(
        status_right("Wi-Fi", "15:24", ""),
        "Wi-Fi  15:24",
        "no battery reading: the clock ends the bar"
    );
}

#[test]
fn numbers_sit_next_to_each_service_and_widen_the_grid_cells() {
    use endora_terminal::presentation::{dash_rows, home};
    let mut value = dash();
    value["groups"][0]["services"][0]["detail"] = json!("2 pending | 17 approved");
    let (text, _) = home(&value, "12:30", 0, 80);
    assert!(
        text.lines()
            .any(|r| r.contains(">a + Overseerr  2 pending, 17 approved")),
        "{text}"
    );
    // The wider cell leaves room for fewer columns than the same group without numbers.
    assert_eq!(dash_rows(&dash(), 40)[0].len(), 2);
    assert_eq!(dash_rows(&value, 40)[0].len(), 1);
    assert!(text.lines().all(|r| r.chars().count() <= 80), "{text}");
}

#[test]
fn home_cursor_moves_across_and_between_rows_and_groups() {
    use endora_terminal::presentation::{Move, dash_move, dash_rows};
    let rows = dash_rows(&dash(), 40);
    assert_eq!(rows, vec![vec![0, 1], vec![2, 3], vec![4]]);
    assert_eq!(dash_move(&rows, 1, Move::Down), 3);
    assert_eq!(dash_move(&rows, 3, Move::Down), 4);
    assert_eq!(dash_move(&rows, 4, Move::Up), 2);
    assert_eq!(dash_move(&rows, 0, Move::Up), 0);
    assert_eq!(dash_move(&rows, 4, Move::Down), 4);
    assert_eq!(dash_move(&rows, 3, Move::Right), 4);
    assert_eq!(dash_move(&rows, 4, Move::Left), 3);
    assert_eq!(dash_move(&rows, 0, Move::Left), 0);
    // A narrow screen drops to one column.
    assert!(dash_rows(&dash(), 10).iter().all(|r| r.len() == 1));
}

#[test]
fn lock_box_is_centered_with_dots_and_no_passcode_text() {
    use endora_terminal::presentation::lock_box;
    use ratatui::layout::Rect;
    let area = Rect::new(0, 0, 80, 30);
    let rect = lock_box(area);
    assert_eq!(rect.x * 2 + rect.width, 80);
    assert_eq!(rect.y * 2 + rect.height, 30);
    assert!(rect.width >= 30 && rect.height >= 7);
    let tiny = lock_box(Rect::new(0, 0, 20, 5));
    assert!(tiny.width <= 20 && tiny.height <= 5);
}

#[test]
fn the_music_page_shows_what_is_playing_the_list_and_what_to_do_next() {
    use endora_terminal::{
        presentation::music_view,
        spotify::{Item, NowPlaying},
    };
    let now = NowPlaying {
        title: "Fictional Song".into(),
        artist: "Artist A".into(),
        playing: true,
        device: "Speaker".into(),
        volume: Some(60),
        progress_ms: 42000,
        duration_ms: 195000,
        uri: "spotify:track:s1".into(),
        artist_uri: "spotify:artist:a1".into(),
    };
    let items = vec![
        Item {
            uri: "spotify:track:t1".into(),
            label: "One - Artist".into(),
            track: true,
        },
        Item {
            uri: "spotify:track:t2".into(),
            label: "Two - Artist".into(),
            track: true,
        },
    ];
    let text = music_view(Some(&now), "Search results", &items, 1, true, "", false);
    assert!(text.contains("Fictional Song - Artist A"), "{text}");
    assert!(
        text.contains("Playing") && text.contains("0:42 / 3:15") && text.contains("vol 60%"),
        "{text}"
    );
    assert!(text.contains("Search results"), "{text}");
    assert!(
        text.contains("   One - Artist") && text.contains(" > Two - Artist"),
        "{text}"
    );
    assert!(
        text.contains("Enter plays") && text.contains("Esc back"),
        "a loaded list says how to play and how to get back: {text}"
    );
    assert!(!text.contains(" | "), "no run-together key lists: {text}");
    // Paused, with nothing listed, and with a note.
    let paused = NowPlaying {
        playing: false,
        ..now.clone()
    };
    let text = music_view(
        Some(&paused),
        "",
        &[],
        0,
        true,
        "No active Spotify device",
        false,
    );
    assert!(
        text.contains("Paused") && text.contains("No active Spotify device"),
        "{text}"
    );
    // Nothing playing, and not set up at all.
    assert!(music_view(None, "", &[], 0, true, "", false).contains("Nothing is playing"));
    let setup = music_view(None, "", &[], 0, false, "", false);
    assert!(
        setup.contains("Press Y") && setup.contains("music setup"),
        "sign in right here, or from a shell: {setup}"
    );
}

#[test]
fn the_bluetooth_page_shows_state_keys_devices_and_a_pairing_countdown() {
    use endora_terminal::{bluetooth::Device, presentation::bluetooth_view};
    let d = |mac: &str, name: &str| Device {
        mac: mac.into(),
        name: name.into(),
    };
    let paired = vec![
        d("AA:BB:CC:DD:EE:01", "Fictional Mac"),
        d("AA:BB:CC:DD:EE:02", "Fictional Speaker"),
    ];
    let connected = vec![d("AA:BB:CC:DD:EE:01", "Fictional Mac")];
    let text = bluetooth_view(true, &connected, &paired, &[], None, 1, "");
    assert!(
        text.contains("Bluetooth is on") && text.contains("connected: Fictional Mac"),
        "{text}"
    );
    for key in [
        "P pair a new device",
        "keeps the ones already paired",
        "S find a speaker",
        "F clear all pairings",
        "R reset",
        "T turn",
    ] {
        assert!(text.contains(key), "{key}: {text}");
    }
    assert!(text.contains("Fictional Mac  (connected)"), "{text}");
    assert!(text.contains(" > Fictional Speaker"), "{text}");
    assert!(
        text.contains("Enter connects") && text.contains("X forgets"),
        "{text}"
    );
    // While a pairing window is open the page shows how to cancel it and how long is left.
    let open = bluetooth_view(true, &[], &[], &[], Some(87), 0, "Old pairings cleared");
    assert!(
        open.contains("DISCOVERABLE as \"Pi Speaker\"") && open.contains("87 s"),
        "{open}"
    );
    assert!(
        open.contains("Esc cancels") && open.contains("Old pairings cleared"),
        "{open}"
    );
    // Off, and a list of speakers found by a scan.
    let off = bluetooth_view(false, &[], &[], &[], None, 0, "");
    assert!(off.contains("Bluetooth is off"), "{off}");
    let found = bluetooth_view(
        true,
        &[],
        &[],
        &[d("AA:BB:CC:DD:EE:03", "Found Speaker")],
        None,
        0,
        "",
    );
    assert!(
        found.contains("Found Speaker") && found.contains("Enter pairs"),
        "{found}"
    );
}

#[test]
fn the_bluetooth_page_never_claims_that_pairing_clears_other_devices() {
    use endora_terminal::presentation::bluetooth_view;
    let text = bluetooth_view(true, &[], &[], &[], None, 0, "");
    assert!(!text.to_lowercase().contains("clears old"), "{text}");
    assert!(text.contains("keeps the ones already paired"), "{text}");
}

#[test]
fn the_bluetooth_page_offers_a_sound_test() {
    let text = endora_terminal::presentation::bluetooth_view(true, &[], &[], &[], None, 0, "");
    assert!(text.contains("A sound test"), "{text}");
}

#[test]
fn the_music_page_is_a_menu_like_the_home_screen_with_one_choice_per_line() {
    use endora_terminal::presentation::{MUSIC_MENU, music_view};
    let text = music_view(None, "", &[], 2, true, "", false);
    for (key, label) in MUSIC_MENU {
        let row = text
            .lines()
            .find(|l| l.trim_end().ends_with(label))
            .unwrap_or_else(|| panic!("{label} missing: {text}"));
        assert!(
            row.contains(&key.to_ascii_uppercase().to_string()),
            "{label} shows its key {key}: {row}"
        );
    }
    let cursor = text.lines().find(|l| l.contains('>')).unwrap();
    assert!(
        cursor.contains(MUSIC_MENU[2].1),
        "cursor on the selected row: {cursor}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("Space") && l.contains("Play or pause")),
        "playback keys are listed one per line: {text}"
    );
    assert!(!text.contains(" | "), "no run-together key lists: {text}");
    assert!(
        text.lines().all(|l| l.chars().count() <= 56),
        "fits the large text size"
    );
    let labels: Vec<&str> = MUSIC_MENU.iter().map(|m| m.1).collect();
    for wanted in [
        "Search",
        "Discover",
        "Smart shuffle (Liked songs)",
        "Play on another device",
        "Playlists",
        "Liked songs",
        "Top artists",
    ] {
        assert!(labels.contains(&wanted), "{wanted}");
    }
}

#[test]
fn the_passcode_line_is_hidden_until_it_is_asked_to_be_shown() {
    use endora_terminal::presentation::passcode_line;
    assert_eq!(passcode_line("Abc9", false), "****");
    assert_eq!(passcode_line("", false), "");
    assert_eq!(
        passcode_line("Abc9", true),
        "Abc9",
        "shows case exactly as typed"
    );
    let long = "x".repeat(40);
    assert_eq!(
        passcode_line(&long, false).chars().count(),
        24,
        "dots are capped"
    );
    let shown = passcode_line(&format!("{long}END"), true);
    assert!(
        shown.chars().count() <= 30 && shown.ends_with("END"),
        "{shown}"
    );
}

#[test]
fn only_a_button_press_counts_as_a_trackpad_click() {
    use endora_terminal::wheel::click_event;
    let mut event = [0u8; 24];
    event[16..18].copy_from_slice(&1u16.to_ne_bytes());
    event[18..20].copy_from_slice(&272u16.to_ne_bytes());
    event[20..24].copy_from_slice(&1i32.to_ne_bytes());
    assert!(click_event(&event), "button down");
    event[20..24].copy_from_slice(&0i32.to_ne_bytes());
    assert!(!click_event(&event), "release is not a second click");
    event[20..24].copy_from_slice(&1i32.to_ne_bytes());
    event[18..20].copy_from_slice(&273u16.to_ne_bytes());
    assert!(!click_event(&event), "another button");
    event[16..18].copy_from_slice(&2u16.to_ne_bytes());
    event[18..20].copy_from_slice(&272u16.to_ne_bytes());
    assert!(!click_event(&event), "motion is not a click");
    assert!(!click_event(&[0; 12]), "short record");
}

#[test]
fn the_lock_box_warns_when_caps_lock_is_on_and_otherwise_shows_the_network_note() {
    use endora_terminal::presentation::lock_notice;
    let warning = "CAPS LOCK ON - Alt+C clears it";
    assert_eq!(lock_notice(true, Some("Joining Wi-Fi...")), warning);
    assert_eq!(lock_notice(true, None), warning);
    assert!(warning.chars().count() <= 32, "fits the lock box");
    assert_eq!(
        lock_notice(false, Some("Joining Wi-Fi...")),
        "Joining Wi-Fi..."
    );
    assert_eq!(lock_notice(false, None), "");
}

#[test]
fn only_the_right_button_press_and_release_are_hold_events() {
    use endora_terminal::wheel::right_button_event;
    let mut event = [0u8; 24];
    event[16..18].copy_from_slice(&1u16.to_ne_bytes());
    event[18..20].copy_from_slice(&273u16.to_ne_bytes());
    event[20..24].copy_from_slice(&1i32.to_ne_bytes());
    assert_eq!(right_button_event(&event), Some(true), "pressed");
    event[20..24].copy_from_slice(&0i32.to_ne_bytes());
    assert_eq!(right_button_event(&event), Some(false), "released");
    event[20..24].copy_from_slice(&2i32.to_ne_bytes());
    assert_eq!(
        right_button_event(&event),
        None,
        "auto-repeat is not a new press"
    );
    event[20..24].copy_from_slice(&1i32.to_ne_bytes());
    event[18..20].copy_from_slice(&272u16.to_ne_bytes());
    assert_eq!(
        right_button_event(&event),
        None,
        "the left button is the click"
    );
    assert_eq!(right_button_event(&[0; 12]), None);
}

#[test]
fn a_hold_is_timed_from_the_press_and_forgotten_on_release() {
    use endora_terminal::wheel::{HoldTimer, RESTART_HOLD};
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let mut timer = HoldTimer::default();
    assert_eq!(timer.held(start), None, "not pressed");
    timer.set(true, start);
    timer.set(true, start + Duration::from_secs(1)); // a repeat does not restart the clock
    assert_eq!(
        timer.held(start + Duration::from_secs(2)),
        Some(Duration::from_secs(2))
    );
    assert!(timer.held(start + RESTART_HOLD).unwrap() >= RESTART_HOLD);
    timer.set(false, start + Duration::from_secs(2));
    assert_eq!(
        timer.held(start + Duration::from_secs(5)),
        None,
        "released early: nothing happens"
    );
    assert!(
        RESTART_HOLD >= Duration::from_secs(3),
        "long enough not to be an accident"
    );
}

#[test]
fn restarting_the_device_uses_a_clean_system_reboot() {
    use endora_terminal::power::reboot_command;
    assert_eq!(
        reboot_command(),
        ("sudo", vec!["-n", "systemctl", "reboot"])
    );
}

#[test]
fn the_discover_radio_says_how_to_like_and_skip() {
    use endora_terminal::{presentation::music_view, spotify::Item};
    let items = vec![Item {
        uri: "spotify:track:t1".into(),
        label: "One - Artist".into(),
        track: true,
    }];
    let radio = music_view(None, "Discover radio", &items, 0, true, "", true);
    assert!(
        radio.lines().any(|l| l.contains('H') && l.contains("Like")),
        "{radio}"
    );
    assert!(
        radio.lines().any(|l| l.contains('X') && l.contains("Skip")),
        "{radio}"
    );
    let plain = music_view(None, "Liked songs", &items, 0, true, "", false);
    assert!(!plain.contains("Like (save"), "{plain}");
}

#[test]
fn liking_and_disliking_the_current_song_is_always_on_the_music_page() {
    use endora_terminal::presentation::music_view;
    let text = music_view(None, "", &[], 0, true, "", false);
    assert!(
        text.lines()
            .any(|l| l.contains('H') && l.contains("Like this song")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains('X') && l.contains("Dislike")),
        "{text}"
    );
    assert!(
        text.lines().all(|l| l.chars().count() <= 56),
        "fits the large text size"
    );
}
