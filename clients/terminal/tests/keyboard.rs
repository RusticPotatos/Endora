use endora_terminal::keyboard::caps_lock_in;
use std::fs;

fn led(root: &std::path::Path, name: &str, value: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("brightness"), value).unwrap();
}

#[test]
fn caps_lock_is_read_from_the_keyboard_led() {
    let root = std::env::temp_dir().join(format!("endora-caps-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    assert!(!caps_lock_in(&root), "no keyboard lights: not on");
    led(&root, "input5::scrolllock", "1\n");
    led(&root, "input5::numlock", "1\n");
    assert!(!caps_lock_in(&root), "other lights do not count");
    led(&root, "input5::capslock", "0\n");
    assert!(!caps_lock_in(&root), "caps light off");
    led(&root, "input9::capslock", "1\n");
    assert!(
        caps_lock_in(&root),
        "any caps light on means caps lock is on"
    );
    led(&root, "input9::capslock", "garbage");
    assert!(!caps_lock_in(&root), "unreadable value is treated as off");
    assert!(
        !caps_lock_in(&root.join("missing")),
        "missing folder is off"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clearing_caps_lock_uses_the_console_led_tool_on_the_keyboard_flag() {
    use endora_terminal::keyboard::caps_off_command;
    // -F changes the keyboard's own flag (what makes capitals), not just the light.
    assert_eq!(caps_off_command(), ("setleds", vec!["-F", "-caps"]));
}

fn key_record(kind: u16, code: u16, value: i32) -> [u8; 24] {
    let mut record = [0u8; 24];
    record[16..18].copy_from_slice(&kind.to_ne_bytes());
    record[18..20].copy_from_slice(&code.to_ne_bytes());
    record[20..24].copy_from_slice(&value.to_ne_bytes());
    record
}

#[test]
fn the_two_keys_left_of_the_trackpad_are_volume_down_and_up() {
    use endora_terminal::keyboard::volume_key_step;
    // Left key (sends Caps Lock) is volume down; the BlackBerry key (sends Super) is volume up.
    assert_eq!(volume_key_step(&key_record(1, 58, 1)), Some(-1), "press");
    assert_eq!(
        volume_key_step(&key_record(1, 58, 2)),
        Some(-1),
        "held: repeats"
    );
    assert_eq!(volume_key_step(&key_record(1, 125, 1)), Some(1), "press");
    assert_eq!(
        volume_key_step(&key_record(1, 125, 2)),
        Some(1),
        "held: repeats"
    );
    assert_eq!(
        volume_key_step(&key_record(1, 58, 0)),
        None,
        "release does nothing"
    );
    assert_eq!(
        volume_key_step(&key_record(1, 30, 1)),
        None,
        "a letter is not volume"
    );
    assert_eq!(
        volume_key_step(&key_record(2, 58, 1)),
        None,
        "not a key event"
    );
    assert_eq!(volume_key_step(&[0; 12]), None);
}

#[test]
fn the_console_stops_treating_those_keys_as_caps_lock_and_super() {
    use endora_terminal::keyboard::{volume_command, volume_keymap};
    let keymap = volume_keymap();
    // Every layer of both keys is cleared, so Shift or Alt plus the key cannot toggle anything.
    for modifiers in [
        "plain",
        "shift",
        "altgr",
        "shift altgr",
        "control",
        "alt",
        "shift control",
        "control alt",
        "shift alt",
    ] {
        for code in [58, 125] {
            let line = format!("{modifiers} keycode {code} = VoidSymbol");
            assert!(keymap.lines().any(|l| l == line), "missing: {line}");
        }
    }
    assert!(
        keymap
            .lines()
            .all(|l| l.contains("keycode 58 =") || l.contains("keycode 125 =")),
        "only those two keys change"
    );
    assert_eq!(
        volume_command(1),
        (
            "wpctl",
            vec!["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"]
        )
    );
    assert_eq!(
        volume_command(-2),
        (
            "wpctl",
            vec!["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "10%-"]
        )
    );
    assert_eq!(
        volume_command(9).1.last(),
        Some(&"20%+"),
        "a big burst is capped"
    );
}

#[test]
fn the_current_volume_is_read_from_wpctl_output() {
    use endora_terminal::keyboard::parse_volume;
    assert_eq!(parse_volume("Volume: 0.45\n"), Some(45));
    assert_eq!(parse_volume("Volume: 1.00"), Some(100));
    assert_eq!(parse_volume("Volume: 0.40 [MUTED]"), Some(40));
    assert_eq!(parse_volume(""), None);
    assert_eq!(parse_volume("Could not connect"), None);
}

#[test]
fn the_keymap_is_loaded_with_root_rights_because_changing_modifier_keys_needs_them() {
    use endora_terminal::keyboard::keymap_command;
    assert_eq!(keymap_command(), ("sudo", vec!["-n", "loadkeys", "-"]));
}
