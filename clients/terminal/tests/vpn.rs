use endora_terminal::vpn::{
    Action, Facts, HomeMatch, Vpn, decide, fingerprint, home_match, parse_warp_status,
    should_learn_home, status_icons,
};

fn facts(vpn: Vpn, home: HomeMatch, internet: bool, node: bool, ticks: u32) -> Facts {
    Facts {
        vpn,
        home,
        internet,
        node_reachable: node,
        unreachable_ticks: ticks,
        manual_hold: false,
    }
}

#[test]
fn warp_status_text_is_parsed_and_unknown_text_is_not_guessed() {
    assert_eq!(
        parse_warp_status("Status update: Connected\nNetwork: healthy"),
        Vpn::Connected
    );
    assert_eq!(
        parse_warp_status("Status update: Disconnected\nReason: Manual Disconnection"),
        Vpn::Disconnected
    );
    assert_eq!(
        parse_warp_status("Status update: Connecting\nReason: Registration"),
        Vpn::Connecting
    );
    assert_eq!(
        parse_warp_status(
            "Status update: Unable\nReason: Registration Missing due to: Daemon Startup"
        ),
        Vpn::Unregistered
    );
    assert_eq!(parse_warp_status("garbage"), Vpn::Unavailable);
    assert_eq!(parse_warp_status(""), Vpn::Unavailable);
}

#[test]
fn it_connects_only_away_from_home_and_never_at_home() {
    use Action::{Connect, Disconnect, Nothing};
    use HomeMatch::{Away, Home, Unknown};
    // Away, online, the node cannot be reached directly for a while: connect.
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Away, true, false, 3)),
        Connect
    );
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Unknown, true, false, 3)),
        Connect
    );
    // Not yet sure, no internet (captive portal), or the node is reachable: wait.
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Away, true, false, 2)),
        Nothing
    );
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Away, false, false, 9)),
        Nothing
    );
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Away, true, true, 9)),
        Nothing
    );
    // At home it never connects, and drops a tunnel that is up so the LAN keeps working.
    assert_eq!(
        decide(&facts(Vpn::Disconnected, Home, true, false, 9)),
        Nothing
    );
    assert_eq!(
        decide(&facts(Vpn::Connected, Home, true, true, 0)),
        Disconnect
    );
    assert_eq!(
        decide(&facts(Vpn::Connecting, Home, true, false, 0)),
        Disconnect
    );
    // Away and already connected: leave it alone. No client or no registration: do nothing.
    assert_eq!(decide(&facts(Vpn::Connected, Away, true, true, 0)), Nothing);
    assert_eq!(
        decide(&facts(Vpn::Unavailable, Away, true, false, 9)),
        Nothing
    );
    assert_eq!(
        decide(&facts(Vpn::Unregistered, Away, true, false, 9)),
        Nothing
    );
}

#[test]
fn home_is_learned_from_a_direct_connection_and_matched_by_fingerprint() {
    let home = fingerprint("aa:bb:cc:dd:ee:ff");
    assert_eq!(
        home,
        fingerprint("AA:BB:CC:DD:EE:FF"),
        "case does not matter"
    );
    assert_ne!(home, fingerprint("aa:bb:cc:dd:ee:00"));
    assert!(!home.contains("aa:bb"), "the address itself is not stored");
    assert_eq!(home_match(Some(&home), Some(&home)), HomeMatch::Home);
    assert_eq!(home_match(Some(&home), Some("0123")), HomeMatch::Away);
    assert_eq!(home_match(None, Some(&home)), HomeMatch::Unknown);
    assert_eq!(home_match(Some(&home), None), HomeMatch::Unknown);
    // Learn only when nothing is stored, the tunnel is down and the node answers directly.
    assert!(should_learn_home(Vpn::Disconnected, true, None, Some("x")));
    assert!(!should_learn_home(Vpn::Connected, true, None, Some("x")));
    assert!(!should_learn_home(
        Vpn::Disconnected,
        false,
        None,
        Some("x")
    ));
    assert!(!should_learn_home(
        Vpn::Disconnected,
        true,
        Some("y"),
        Some("x")
    ));
    assert!(!should_learn_home(Vpn::Disconnected, true, None, None));
}

#[test]
fn top_bar_icons_show_bluetooth_the_tunnel_and_wifi_state() {
    assert_eq!(
        status_icons("Internet available", Vpn::Disconnected, false),
        "Wi-Fi"
    );
    assert_eq!(
        status_icons("Internet available", Vpn::Connected, false),
        "VPN  Wi-Fi"
    );
    assert_eq!(
        status_icons("Internet available", Vpn::Connecting, false),
        "VPN..  Wi-Fi"
    );
    assert_eq!(
        status_icons("Portal login needed", Vpn::Disconnected, false),
        "Wi-Fi!"
    );
    assert_eq!(
        status_icons("Connected; checking internet", Vpn::Unavailable, false),
        "Wi-Fi!"
    );
    assert_eq!(
        status_icons("Joining", Vpn::Disconnected, false),
        "Wi-Fi..."
    );
    assert_eq!(
        status_icons("No Wi-Fi", Vpn::Connected, false),
        "VPN  No Wi-Fi"
    );
    assert_eq!(
        status_icons("NetworkManager unavailable", Vpn::Unregistered, false),
        "No Wi-Fi"
    );
    // A connected Bluetooth device comes first.
    assert_eq!(
        status_icons("Internet available", Vpn::Disconnected, true),
        "BT  Wi-Fi"
    );
    assert_eq!(
        status_icons("Internet available", Vpn::Connected, true),
        "BT  VPN  Wi-Fi"
    );
}
