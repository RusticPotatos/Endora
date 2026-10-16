use endora_terminal::{network::portal_offer, presentation::lock_hint};

#[test]
fn the_text_login_is_offered_once_per_portal_and_again_after_the_network_changes() {
    let mut offered = false;
    assert!(!portal_offer(&mut offered, "Internet available"));
    assert!(portal_offer(&mut offered, "Portal login needed"));
    assert!(
        !portal_offer(&mut offered, "Portal login needed"),
        "no nagging while it is open"
    );
    assert!(!portal_offer(&mut offered, "Portal login needed"));
    // Once the network is no longer a portal, a later portal is offered again.
    assert!(!portal_offer(&mut offered, "Internet available"));
    assert!(portal_offer(&mut offered, "Portal login needed"));
    // A brief state flicker that is not a portal does not re-arm a declined offer.
    let mut declined = true;
    assert!(!portal_offer(&mut declined, "Connected; checking internet"));
    assert!(!portal_offer(&mut declined, "Portal login needed"));
}

#[test]
fn the_lock_screen_says_how_to_get_online_when_there_is_no_network() {
    assert_eq!(
        lock_hint("No Wi-Fi"),
        Some("No network | Esc opens Wi-Fi settings")
    );
    assert_eq!(
        lock_hint("Portal login needed"),
        Some("Wi-Fi login needed | Esc, then L")
    );
    assert_eq!(lock_hint("Joining"), Some("Joining Wi-Fi..."));
    assert_eq!(lock_hint("Internet available"), None);
    assert_eq!(lock_hint("Connected; checking internet"), None);
}
