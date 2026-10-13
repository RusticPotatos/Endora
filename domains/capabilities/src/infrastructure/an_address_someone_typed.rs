use super::as_url;

#[test]
fn a_bare_host_and_port_becomes_a_url() {
    // Live, and invisible: this is exactly what was in the settings, and every request
    // built from it failed with `http: invalid format` — taking presence, live states
    // and the standing-trouble watch down without anything appearing to be broken.
    assert_eq!(as_url("192.168.1.10:8123"), "http://192.168.1.10:8123");
    assert_eq!(
        as_url("  homeassistant.local:8123  "),
        "http://homeassistant.local:8123"
    );
}

#[test]
fn a_scheme_someone_gave_is_never_overridden() {
    // Someone who needs TLS has typed why, and guessing over them would be worse than
    // the bug this fixes.
    assert_eq!(as_url("https://ha.example.com"), "https://ha.example.com");
    assert_eq!(
        as_url("http://192.168.1.10:8123/"),
        "http://192.168.1.10:8123"
    );
}

#[test]
fn nothing_typed_stays_nothing() {
    // An unset address must remain unset, so "you have not configured this" keeps
    // being the message rather than a request to `http://`.
    assert_eq!(as_url(""), "");
    assert_eq!(as_url("   "), "");
}
