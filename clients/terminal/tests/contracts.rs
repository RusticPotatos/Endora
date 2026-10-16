use endora_terminal::{
    protocol::{Sse, clean},
    storage::{Config, Paths},
};

#[test]
fn fragmented_sse_and_unknown_events() {
    let mut sse = Sse::default();
    assert!(sse.push(b"data: {\"type\":\"tok").unwrap().is_empty());
    let events = sse
        .push(b"en\",\"text\":\"hello\"}\r\n\r\ndata: {\"type\":\"future\"}\n\n")
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["text"], "hello");
    assert!(sse.push(b": heartbeat\n\n").unwrap().is_empty());
}

#[test]
fn terminal_controls_are_removed() {
    assert_eq!(clean("hello\x1b[2J\x07\nworld"), "hello[2J\nworld");
}

#[test]
fn caps_lock_does_not_change_shortcuts_or_lowercase_chat_text() {
    use crossterm::event::KeyCode;
    use endora_terminal::protocol::shortcut;
    assert_eq!(shortcut(KeyCode::Char('S'), true), KeyCode::Char('s'));
    assert_eq!(shortcut(KeyCode::Char('Q'), true), KeyCode::Char('q'));
    assert_eq!(shortcut(KeyCode::Char('S'), false), KeyCode::Char('S'));
}

#[test]
fn transport_choice_is_explicit() {
    let mut config = Config {
        url: "http://192.168.1.10:8787".into(),
        allow_http: false,
        ca_file: None,
    };
    assert!(config.validate().is_err());
    config.allow_http = true;
    assert!(config.validate().is_ok());
    config.url = "https://user:password@example.com".into();
    assert!(config.validate().is_err());
}

#[test]
fn private_files_and_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let paths = Paths::at(root.clone());
    paths.write("token", b"fictional-session").unwrap();
    let file = root.join("token");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(paths.read("token").is_err());
    std::fs::remove_file(&file).unwrap();
    symlink(root.join("other"), &file).unwrap();
    assert!(paths.write("token", b"secret").is_err());
    std::fs::remove_dir_all(root).unwrap();
}
