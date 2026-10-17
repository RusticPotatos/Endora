//! A container's log reaches a team member's model only scrubbed (ADR 0106): every secret
//! shape Endora knows is replaced, and the rest of the line is kept, so the log still says
//! what went wrong.

use super::scrub_secrets;

#[test]
fn the_shapes_the_tripwire_knows_are_replaced_and_the_line_kept() {
    let scrubbed = scrub_secrets(
        "10:00:01 starting with sk-abcdefabcdefabcdefabcdef and ghp_1234567890abcdefghijklmnopqrstuv\n",
    );
    assert_eq!(
        scrubbed,
        "10:00:01 starting with [redacted] and [redacted]\n"
    );
    let jwt = scrub_secrets("cookie for https://example.com/?t=eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM");
    assert!(!jwt.contains("eyJ"), "{jwt}");
    assert!(jwt.contains("https://example.com/"), "{jwt}");
}

#[test]
fn a_value_given_to_a_secret_name_is_replaced_whatever_its_shape() {
    for (line, kept) in [
        ("DB_PASSWORD=hunter2 user=jane", "user=jane"),
        (
            "connecting with password: hunter2",
            "connecting with password:",
        ),
        (
            "{\"api_key\": \"0123abcd\", \"port\": 8989}",
            "\"port\": 8989",
        ),
        ("X-Api-Key: 0123abcd", "X-Api-Key:"),
        ("client_secret='s3cr3t/with:colons' ok", "ok"),
        ("access_token=abc&page=2", "page=2"),
    ] {
        let scrubbed = scrub_secrets(line);
        assert!(
            !scrubbed.contains("hunter2")
                && !scrubbed.contains("0123abcd")
                && !scrubbed.contains("s3cr3t")
                && !scrubbed.contains("abc&"),
            "{line} -> {scrubbed}"
        );
        assert!(scrubbed.contains("[redacted]"), "{line} -> {scrubbed}");
        assert!(scrubbed.contains(kept), "{line} -> {scrubbed}");
    }
}

#[test]
fn what_follows_bearer_or_basic_is_replaced() {
    let scrubbed = scrub_secrets("GET /api 401 Authorization: Bearer abc.def-123\n");
    assert_eq!(scrubbed, "GET /api 401 Authorization: Bearer [redacted]\n");
    let basic = scrub_secrets("sent basic dXNlcjpwYXNz to the proxy");
    assert_eq!(basic, "sent basic [redacted] to the proxy");
}

#[test]
fn a_private_keys_whole_block_is_replaced() {
    let scrubbed = scrub_secrets(
        "loading key\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\nAAAAB3NzaC1\n-----END OPENSSH PRIVATE KEY-----\nloaded\n",
    );
    assert_eq!(scrubbed, "loading key\n[redacted: a private key]\nloaded\n");
    let cut = scrub_secrets("-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nand the log ends");
    assert_eq!(
        cut, "[redacted: a private key]\n",
        "a block cut short hides the rest"
    );
}

#[test]
fn ordinary_log_lines_are_left_alone() {
    for line in [
        "2026-10-17T10:00:00Z INFO sonarr: Import complete for Some Show S01E02\n",
        "WARN monkey=3 bypass=true tokens=500 keyboard: ok",
    ] {
        assert_eq!(scrub_secrets(line), line);
    }
}
