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

// The node's wire types (ADR 0099). Each sample is what the node sends; rendering it must show
// what it carries, so a field renamed on one side fails here as well as in the node's own tests.

#[test]
fn every_chat_stream_event_the_node_sends_shows() {
    use endora_terminal::presentation::on_stream;
    let mut chat = String::new();
    let mut seen = false;
    let mut statuses = Vec::new();
    for event in endora_protocol::samples::stream() {
        if let Some(status) = on_stream(&event, &mut chat, &mut seen) {
            statuses.push(status);
        }
    }
    assert!(
        statuses.iter().any(|s| s.contains("Checking the weather")),
        "a step says what Endora is doing: {statuses:?}"
    );
    assert!(
        statuses
            .iter()
            .any(|s| s.contains("The model did not answer")),
        "an error is shown, not swallowed: {statuses:?}"
    );
    assert_eq!(
        chat.matches("It is sunny.").count(),
        1,
        "the streamed text is shown once, not again when the reply completes: {chat:?}"
    );
}

#[test]
fn every_handheld_page_shows_what_the_node_sends() {
    use endora_terminal::presentation::display;
    for (page, expected) in [
        (endora_protocol::samples::home(), ["Example lamp", "21.5"]),
        (
            endora_protocol::samples::media(),
            ["Example Film (2024)", "Availability"],
        ),
        (
            endora_protocol::samples::jobs(),
            ["Check example", "succeeded"],
        ),
    ] {
        let text = display(&serde_json::to_value(&page).unwrap(), 0);
        for shown in expected {
            assert!(text.contains(shown), "{shown} is missing from {text}");
        }
    }
}

#[test]
fn the_home_screen_shows_the_nodes_services() {
    let dashboard = serde_json::to_value(endora_protocol::samples::dashboard()).unwrap();
    let (text, _) = endora_terminal::presentation::home(&dashboard, "12:00", 0, 80);
    assert!(text.contains("Example Server"), "{text}");
    let services = endora_terminal::presentation::dash_services(&dashboard);
    assert!(services.iter().any(|s| s["name"] == "Example Server"));
}
