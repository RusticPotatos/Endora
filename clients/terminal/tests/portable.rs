use endora_terminal::{
    network::parse_wifi,
    power::{Mode, Power, Stage},
    vault,
};
use std::time::Duration;

#[test]
fn four_character_device_code_is_accepted_and_shorter_codes_are_rejected() {
    let envelope = vault::seal("4826", "https://node.example.com", b"fictional-session").unwrap();
    assert_eq!(
        &*vault::open("4826", "https://node.example.com", &envelope).unwrap(),
        b"fictional-session"
    );
    assert!(vault::seal("123", "https://node.example.com", b"fictional-session").is_err());
    assert!(vault::seal("éé", "https://node.example.com", b"fictional-session").is_err());
}

#[test]
fn vault_authenticates_passphrase_and_origin_and_never_contains_token() {
    let bytes = vault::seal(
        "correct horse battery staple",
        "https://node.example.com",
        b"fictional-privileged-session",
    )
    .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("fictional-privileged-session"));
    assert_eq!(
        &*vault::open(
            "correct horse battery staple",
            "https://node.example.com",
            &bytes
        )
        .unwrap(),
        b"fictional-privileged-session"
    );
    assert!(vault::open("wrong", "https://node.example.com", &bytes).is_err());
    assert!(
        vault::open(
            "correct horse battery staple",
            "https://other.example.com",
            &bytes
        )
        .is_err()
    );
    let mut changed = bytes;
    *changed.last_mut().unwrap() ^= 1;
    assert!(
        vault::open(
            "correct horse battery staple",
            "https://node.example.com",
            &changed
        )
        .is_err()
    );
}

#[test]
fn pocket_wake_is_consumed_and_busy_work_stays_awake() {
    let mut power = Power::default();
    assert_eq!(power.stage(Duration::from_secs(61), false), Stage::Dim);
    assert_eq!(power.stage(Duration::from_secs(181), true), Stage::Awake);
    assert_eq!(power.stage(Duration::from_secs(181), false), Stage::Pocket);
    assert!(power.wake());
    assert!(!power.wake());
    power.mode = Mode::Desk;
    assert_eq!(power.stage(Duration::from_secs(181), false), Stage::Awake);
}

#[test]
fn wifi_picker_preserves_escaped_names_without_shell_interpolation() {
    let rows = parse_wifi("*:Guest\\: WiFi:80:WPA2\n:Cafe:30:--\n:Guest\\: WiFi:70:WPA2\n");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ssid, "Guest: WiFi");
    assert!(rows[0].connected);
    assert!(!rows[1].secured);
}

#[test]
fn idle_learning_is_bounded_and_only_suggests_changes() {
    use endora_terminal::power::Usage;
    let mut usage = Usage::default();
    for _ in 0..8 {
        usage.record(Duration::from_secs(5));
    }
    assert_eq!(usage.suggestion(), Some(300));
    assert_eq!(usage.pocket_seconds, 180);
    usage.apply();
    assert_eq!(usage.pocket_seconds, 300);
    for _ in 0..100 {
        usage.record(Duration::from_secs(3600));
    }
    assert_eq!(usage.samples.len(), 32);
    assert_eq!(usage.suggestion(), Some(120));
}
